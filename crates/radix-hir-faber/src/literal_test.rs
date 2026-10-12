use super::*;
use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_lexer::{Interner, TokenKind};

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

fn render(literal: &HirLiteral, surface: &impl KeywordSurface) -> String {
    let types = radix_types::TypeTable::new();
    let interner = Interner::new();
    let codegen = crate::FaberCodegen::new(&types, &interner);
    let mut writer = CodeWriter::new();
    codegen.write_literal(literal, surface, &mut writer);
    writer.finish()
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
