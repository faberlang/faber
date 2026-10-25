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

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{DefId, HirLiteral, HirObjectField, HirObjectKey};
use radix_lexer::{Symbol, TokenKind};
use radix_types::Primitive;

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

/// Whether a raw `textus` payload cannot be re-lexed inside `"..."`.
///
/// The interned payload keeps escapes as written, so the scan is backslash
/// aware: a backslash consumes the following character, which makes `\"` an
/// escaped quote rather than a terminator. Quotes fail on an unescaped `"`, on
/// a line break, and on `\»`, which is a valid guillemet escape rather than a
/// quote one.
fn textus_needs_guillemets(text: &str) -> bool {
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if chars.next() == Some('»') {
                    return true;
                }
            }
            '"' | '\n' | '\r' => return true,
            _ => {}
        }
    }
    false
}

impl super::FaberCodegen<'_> {
    /// Write one genus/struct field (`field = value`).
    pub(super) fn write_genus_field(
        &self,
        field: &HirObjectField,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_object_field(field, ObjectFieldStyle::Genus, names, surface, writer);
    }

    /// Write one JSON valor / tabula field (`"key": value`).
    pub(super) fn write_json_field(
        &self,
        field: &HirObjectField,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_object_field(field, ObjectFieldStyle::Json, names, surface, writer);
    }

    fn write_object_field(
        &self,
        field: &HirObjectField,
        style: ObjectFieldStyle,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match &field.key {
            HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                match style {
                    ObjectFieldStyle::Genus => {
                        writer.write(&self.symbol_to_string(*name));
                    }
                    ObjectFieldStyle::Json => {
                        self.write_quoted_symbol(*name, writer);
                    }
                }
                self.write_object_field_value(field, style, names, surface, writer);
            }
            HirObjectKey::Computed(expr) => {
                writer.write("[");
                self.write_expr(expr, names, surface, writer);
                writer.write("]");
                self.write_object_field_value(field, style, names, surface, writer);
            }
            HirObjectKey::Spread(expr) => {
                writer.write(surface.keyword(TokenKind::Sparge));
                writer.write(" ");
                self.write_expr(expr, names, surface, writer);
            }
        }
    }

    /// Write the explicit value of an object field, if the field has one.
    fn write_object_field_value(
        &self,
        field: &HirObjectField,
        style: ObjectFieldStyle,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(value) = &field.value {
            writer.write(style.value_separator());
            self.write_expr(value, names, surface, writer);
        }
    }

    /// Write a lowered literal using source-level Faber keywords and sigils.
    pub(super) fn write_literal(
        &self,
        lit: &HirLiteral,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match lit {
            HirLiteral::Int(value) => writer.write(&value.to_string()),
            // The exact value as canonical decimal digits.
            HirLiteral::BigInt(digits) => writer.write(digits),
            // D8.9: the authored digit text is what typing reads for decimal
            // widths, so re-emit it verbatim; an f64 re-spelling could round
            // a `decimal` literal or rewrite `1e-5` as `0.00001`.
            HirLiteral::FloatText { value, text } if !value.is_nan() => {
                writer.write(self.interner.resolve(*text));
            }
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. } => {
                if value.is_nan() {
                    writer.write(surface.contextual_keyword("nonnumerus"));
                } else {
                    writer.write(&self.format_float_literal(*value));
                }
            }
            HirLiteral::String(sym) => self.write_textus_literal(*sym, writer),
            HirLiteral::Ascii(sym) => {
                writer.write("'");
                writer.write(self.interner.resolve(*sym));
                writer.write("'");
            }
            HirLiteral::Octeti(sym) => {
                writer.write("|");
                writer.write(self.interner.resolve(*sym));
                writer.write("|");
            }
            HirLiteral::JsonValor(value) => {
                self.write_json_valor(value, writer);
            }
            HirLiteral::Regex(pattern, _flags) => {
                writer.write(surface.type_surface(TypeBindingKind::Primitive(Primitive::Regex)));
                writer.write(" /");
                self.write_regex_body(*pattern, writer);
                writer.write("/");
            }
            HirLiteral::Bool(value) => {
                writer.write(surface.contextual_keyword(if *value { "verum" } else { "falsum" }));
            }
            HirLiteral::Nil => writer.write(surface.contextual_keyword("nulla")),
        }
    }

    /// Write the raw body of an inline regex literal (`/…/`).
    ///
    /// Ruling d381ae56: the HIR pattern is the raw slash-literal body, so a
    /// `/` whose preceding backslash run is even is written as `\/`, while
    /// `\/` already in the pattern stays unchanged and `\\` counts as an
    /// escaped backslash rather than escaping a following `/`.
    fn write_regex_body(&self, pattern: Symbol, writer: &mut CodeWriter) {
        let text = self.interner.resolve(pattern);
        let mut body = String::with_capacity(text.len());
        let mut escaped = false;
        for ch in text.chars() {
            if escaped {
                body.push(ch);
                escaped = false;
            } else if ch == '\\' {
                body.push(ch);
                escaped = true;
            } else if ch == '/' {
                body.push('\\');
                body.push(ch);
            } else {
                body.push(ch);
            }
        }
        writer.write(&body);
    }

    /// Emit a `textus` literal, preferring guillemets when `"..."` would break.
    pub(super) fn write_textus_literal(&self, sym: Symbol, writer: &mut CodeWriter) {
        let text = self.interner.resolve(sym);
        if textus_needs_guillemets(text) {
            writer.write("«");
            writer.write_raw(text);
            writer.write("»");
        } else {
            self.write_quoted_symbol(sym, writer);
        }
    }

    /// Emit a build-time embed (`insere "path"`, D8.10) as authored.
    pub(super) fn write_insere(
        &self,
        path: Symbol,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.keyword(TokenKind::Insere));
        writer.write(" ");
        self.write_textus_literal(path, writer);
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
    pub(super) fn write_quoted_symbol(&self, sym: Symbol, writer: &mut CodeWriter) {
        writer.write("\"");
        writer.write(self.interner.resolve(sym));
        writer.write("\"");
    }

    /// Round-trip a compile-time JSON valor literal back to bare `{ ... }` JSON
    /// syntax. See `docs/factory/inline-json-valor/contract.md`.
    fn write_json_valor(&self, value: &radix_syntax::JsonValue, writer: &mut CodeWriter) {
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
                        self.write_quoted_symbol(member.key, writer);
                        writer.write(": ");
                        self.write_json_valor(&member.value, writer);
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
                    self.write_json_valor(element, writer);
                }
                writer.write("]");
            }
            JsonValue::String(sym) => self.write_quoted_symbol(*sym, writer),
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
    pub(super) fn write_symbol_literal(&self, symbol: Symbol, writer: &mut CodeWriter) {
        writer.write("\"");
        writer.write(&self.symbol_to_string(symbol));
        writer.write("\"");
    }
}

#[cfg(test)]
#[path = "literal_test.rs"]
mod tests;
