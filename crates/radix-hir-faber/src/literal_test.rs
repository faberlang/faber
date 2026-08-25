use super::*;
use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_lexer::TokenKind;

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, _kind: TokenKind) -> &str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &str {
        "type"
    }
}

struct EnglishSurface;

impl KeywordSurface for EnglishSurface {
    fn keyword(&self, _kind: TokenKind) -> &str {
        "keyword"
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        if key == "nonnumerus" {
            "nan"
        } else {
            key
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &str {
        "type"
    }
}

fn render(literal: HirLiteral, surface: &impl KeywordSurface) -> String {
    let codegen = crate::FaberCodegen::default();
    let interner = Interner::new();
    let mut writer = CodeWriter::new();
    codegen.write_literal(&literal, &interner, surface, &mut writer);
    writer.finish()
}

#[test]
fn non_finite_literals_round_trip_with_faber_spellings() {
    assert_eq!(render(HirLiteral::Float(f64::INFINITY), &LatinSurface), "∞");
    assert_eq!(
        render(HirLiteral::Float(f64::NAN), &LatinSurface),
        "nonnumerus"
    );
    assert_eq!(render(HirLiteral::Float(f64::NAN), &EnglishSurface), "nan");
    assert_eq!(
        render(HirLiteral::Float(f64::NEG_INFINITY), &LatinSurface),
        "-∞"
    );
}

#[test]
fn finite_float_literals_keep_decimal_convention() {
    assert_eq!(render(HirLiteral::Float(7.0), &LatinSurface), "7.0");
    assert_eq!(render(HirLiteral::Float(0.125), &LatinSurface), "0.125");
}
