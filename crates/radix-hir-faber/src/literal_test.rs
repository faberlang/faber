use super::*;
use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_lexer::{Interner, TokenKind};
use radix_types::Primitive;

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

struct EnglishSurface;

impl KeywordSurface for EnglishSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        if key == "nonnumerus" { "nan" } else { key }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

fn render_with_interner(
    literal: &HirLiteral,
    interner: &Interner,
    surface: &impl KeywordSurface,
) -> String {
    let types = radix_types::TypeTable::new();
    let codegen = crate::FaberCodegen::new(&types, interner);
    let mut writer = CodeWriter::new();
    codegen.write_literal(literal, surface, &mut writer);
    writer.finish()
}

fn render(literal: &HirLiteral, surface: &impl KeywordSurface) -> String {
    render_with_interner(literal, &Interner::new(), surface)
}

struct LocaleRegexSurface;

impl KeywordSurface for LocaleRegexSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        key
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &'static str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Regex) => "rgx",
            _ => "type",
        }
    }
}

#[test]
fn non_finite_literals_round_trip_with_faber_spellings() {
    assert_eq!(
        render(&HirLiteral::Float(f64::INFINITY), &LatinSurface),
        "∞"
    );
    assert_eq!(
        render(&HirLiteral::Float(f64::NAN), &LatinSurface),
        "nonnumerus"
    );
    assert_eq!(render(&HirLiteral::Float(f64::NAN), &EnglishSurface), "nan");
    assert_eq!(
        render(&HirLiteral::Float(f64::NEG_INFINITY), &LatinSurface),
        "-∞"
    );
}

#[test]
fn finite_float_literals_keep_decimal_convention() {
    assert_eq!(render(&HirLiteral::Float(7.0), &LatinSurface), "7.0");
    assert_eq!(render(&HirLiteral::Float(0.125), &LatinSurface), "0.125");
}

#[test]
fn regex_literal_emits_slash_form_in_locale_word() {
    let mut interner = Interner::new();
    let escaped = interner.intern(r"\d+\/x");
    let rendered = render_with_interner(
        &HirLiteral::Regex(escaped, None),
        &interner,
        &LocaleRegexSurface,
    );
    assert_eq!(rendered, r"rgx /\d+\/x/");
    assert!(!rendered.contains('↦'));

    let bare = interner.intern("a/b");
    let rendered = render_with_interner(
        &HirLiteral::Regex(bare, None),
        &interner,
        &LocaleRegexSurface,
    );
    assert_eq!(rendered, r"rgx /a\/b/");
    assert!(!rendered.contains('↦'));
}

#[test]
fn object_fields_keep_genus_and_json_separators() {
    let types = radix_types::TypeTable::new();
    let mut interner = Interner::new();
    let key = interner.intern("field");
    let value = radix_hir::HirExpression {
        id: radix_hir::HirId(1),
        kind: radix_hir::HirExpressionKind::Literal(HirLiteral::Int(7)),
        ty: None,
        span: radix_diagnostics::Span::default(),
    };
    let field = HirObjectField {
        key: HirObjectKey::Ident(key),
        value: Some(value),
    };
    let codegen = crate::FaberCodegen::new(&types, &interner);
    let names = FxHashMap::default();

    let mut genus = CodeWriter::new();
    codegen.write_genus_field(&field, &names, &LatinSurface, &mut genus);
    assert_eq!(genus.finish(), "field = 7");

    let mut json = CodeWriter::new();
    codegen.write_json_field(&field, &names, &LatinSurface, &mut json);
    assert_eq!(json.finish(), "\"field\": 7");
}
