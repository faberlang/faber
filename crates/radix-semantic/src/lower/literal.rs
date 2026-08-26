//! Shared AST literal → HIR literal mapping for expression and pattern lowering.
//!
//! This is a pure mapping: one `Literal` variant to one `HirLiteral` variant. It does
//! not validate literal semantics, type-check, or handle forma/capture lowering.

use radix_hir::HirLiteral;
use radix_syntax::Literal;

/// Map a surface literal into the HIR literal taxonomy used by expressions.
pub(crate) fn literal_to_hir(lit: &Literal) -> Result<HirLiteral, &'static str> {
    match lit {
        Literal::Int(value) => Ok(HirLiteral::Int(*value)),
        Literal::Float(value) => Ok(HirLiteral::Float(*value)),
        Literal::FloatText { value, text } => Ok(HirLiteral::FloatText {
            value: *value,
            text: *text,
        }),
        Literal::String(value) => Ok(HirLiteral::String(*value)),
        Literal::Ascii(value) => Ok(HirLiteral::Ascii(*value)),
        Literal::Octeti(value) => Ok(HirLiteral::Octeti(*value)),
        Literal::JsonValor(value) => Ok(HirLiteral::JsonValor(value.clone())),
        Literal::Bool(value) => Ok(HirLiteral::Bool(*value)),
        Literal::Nil => Ok(HirLiteral::Nil),
        Literal::Forma(_) => Err("forma template literals must lower through forma capture"),
    }
}
