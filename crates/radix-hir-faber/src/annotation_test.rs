//! A-57: `HirAnnotationValue::Float` re-emits as a fractus literal.
//!
//! A whole-valued float spelled `7` re-parses as an integer literal and fails
//! against a `fractus` contract field, so the arm must keep the decimal point
//! the way `format_float_literal` does.

use crate::{FaberCodegen, KeywordSurface, TypeBindingKind};
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirAnnotation, HirAnnotationField, HirAnnotationValue};
use radix_lexer::{Interner, Span, TokenKind};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

struct PlainSurface;

impl KeywordSurface for PlainSurface {
    fn keyword(&self, _kind: TokenKind) -> &str {
        "kw"
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &str {
        "ignotum"
    }
}

fn emit_float_field(value: f64) -> String {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let annotation = HirAnnotation::with_fields(
        interner.intern("Meta"),
        vec![HirAnnotationField {
            name: interner.intern("weight"),
            value: HirAnnotationValue::Float(value),
            span: Span::new(0, 0),
        }],
        Span::new(0, 0),
    );
    let codegen = FaberCodegen::new(&types, &interner);
    let mut writer = CodeWriter::new();
    codegen.write_hir_annotation(
        &annotation,
        None,
        0,
        &FxHashMap::default(),
        &PlainSurface,
        &mut writer,
    );
    writer.finish()
}

#[test]
fn whole_valued_float_annotation_field_keeps_decimal_point() {
    assert_eq!(emit_float_field(7.0), "@ Meta { weight = 7.0 }");
}

#[test]
fn fractional_float_annotation_field_is_unchanged() {
    assert_eq!(emit_float_field(1.5), "@ Meta { weight = 1.5 }");
}
