//! Shared AST literal → HIR literal mapping for expression and pattern lowering.
//!
//! This is a pure mapping: one `Literal` variant to one `HirLiteral` variant. It does
//! not validate literal semantics, type-check, or handle forma/capture lowering.

use radix_hir::HirLiteral;
use radix_lexer::Interner;
use radix_syntax::Literal;

/// Map a surface literal into the HIR literal taxonomy used by expressions.
pub(crate) fn literal_to_hir(
    lit: &Literal,
    interner: &Interner,
) -> Result<HirLiteral, &'static str> {
    match lit {
        Literal::Int(value) => Ok(HirLiteral::Int(*value)),
        // Normalised to canonical decimal digits so every later stage reads
        // one spelling (the lexer keeps the authored radix prefix).
        Literal::BigInt(text) => {
            radix_types::big_literal::parse_big_literal(interner.resolve(*text))
                .map(|value| HirLiteral::BigInt(value.to_string().into_boxed_str()))
                .ok_or("malformed integer literal")
        }
        Literal::Float(value) => Ok(HirLiteral::Float(*value)),
        Literal::FloatText { value, text } => Ok(HirLiteral::FloatText {
            value: *value,
            text: *text,
        }),
        Literal::String(value) => Ok(HirLiteral::String(*value)),
        Literal::Ascii(value) => Ok(HirLiteral::Ascii(*value)),
        Literal::Octeti(value) => Ok(HirLiteral::Octeti(*value)),
        Literal::Regex(pattern) => Ok(HirLiteral::Regex(*pattern, None)),
        Literal::JsonValor(value) => Ok(HirLiteral::JsonValor(value.clone())),
        Literal::Bool(value) => Ok(HirLiteral::Bool(*value)),
        Literal::Nil => Ok(HirLiteral::Nil),
        Literal::Forma(_) => Err("forma template literals must lower through forma capture"),
    }
}
