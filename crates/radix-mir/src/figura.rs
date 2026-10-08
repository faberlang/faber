//! Index literal resolution and formatting for MIR.
//!
//! Resolves literal index values from the type table and formats index
//! expressions for diagnostic display. `format_figura` is dead code in this
//! crate but kept for caller convenience in sibling leaf targets.
use radix_lexer::Interner;
use radix_types::{IndexExpr, IndexId, TypeTable};

pub fn i64s_from_literal_index(index_id: IndexId, types: &TypeTable) -> Option<Vec<i64>> {
    radix_types::i64s_from_literal_index(index_id, types)
}

#[allow(dead_code)]
pub fn format_figura(index_id: IndexId, types: &TypeTable, interner: &Interner) -> String {
    format_figura_at_precedence(index_id, types, interner, 0).0
}

fn format_figura_at_precedence(
    index_id: IndexId,
    types: &TypeTable,
    interner: &Interner,
    parent_precedence: u8,
) -> (String, u8) {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => (value.to_string(), 3),
        IndexExpr::Param(symbol) => (interner.resolve(*symbol).to_owned(), 3),
        IndexExpr::Runtime(source) => (format!("<runtime:{source:?}>"), 3),
        IndexExpr::Affine { base, offset } => {
            let (base, _) = format_figura_at_precedence(*base, types, interner, 1);
            let text = match (*offset).cmp(&0) {
                std::cmp::Ordering::Less => format!("{base} - {}", offset.unsigned_abs()),
                std::cmp::Ordering::Greater => format!("{base} + {offset}"),
                std::cmp::Ordering::Equal => base,
            };
            parenthesize_figura(text, 1, parent_precedence)
        }
        IndexExpr::Divide { numerator, divisor } => {
            let (numerator, _) = format_figura_at_precedence(*numerator, types, interner, 2);
            parenthesize_figura(format!("{numerator} / {divisor}"), 2, parent_precedence)
        }
        IndexExpr::Tuple(dims) if dims.is_empty() => ("[]".to_owned(), 3),
        IndexExpr::Tuple(dims) => {
            let parts = dims
                .iter()
                .map(|dim| format_figura_at_precedence(*dim, types, interner, 0).0)
                .collect::<Vec<_>>()
                .join(", ");
            (format!("[{parts}]"), 3)
        }
        IndexExpr::Infer(_) | IndexExpr::Unspecified => ("_".to_owned(), 3),
    }
}

fn parenthesize_figura(text: String, precedence: u8, parent_precedence: u8) -> (String, u8) {
    if precedence < parent_precedence {
        (format!("({text})"), precedence)
    } else {
        (text, precedence)
    }
}
