//! Emitter proofs for the two read forms: `lege` (en `read`) and the
//! line-oriented `lineam` (en `line`).
//!
//! `HirExpressionKind::ReadLine(bool)` carries the authored mode: `false` is
//! the bare whole-input read, `true` is the line-oriented read. They are
//! different operations, so a dropped flag would re-emit `lineam` as bare
//! `lege` and silently change the meaning of the source.

use crate::surface::{KeywordSurface, TypeBindingKind};
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirExpression, HirExpressionKind, HirId};
use radix_lexer::{Interner, TokenKind};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

/// Latin surface: contextual keywords are their canonical Latin spellings.
struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

/// English reader surface mirroring the `en` pack rows (`lege = "read"`,
/// `lineam = "line"`).
struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        match key {
            "lege" => "read",
            "lineam" => "line",
            key => key,
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

fn read_line(is_line: bool) -> HirExpression {
    HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::ReadLine(is_line),
        ty: None,
        span: radix_lexer::Span::new(0, 0),
    }
}

fn render(expr: &HirExpression, surface: &impl KeywordSurface) -> String {
    let interner = Interner::new();
    let names: FxHashMap<radix_types::DefId, radix_lexer::Symbol> = FxHashMap::default();
    let types = TypeTable::new();
    let codegen = crate::FaberCodegen::new(&types, &interner);
    let mut writer = CodeWriter::new();
    codegen.write_expr(expr, &names, surface, &mut writer);
    writer.finish()
}

#[test]
fn lege_lineam_latin_line_read_keeps_lineam() {
    let code = render(&read_line(true), &LatinSurface);
    assert_eq!(code.trim(), "lineam", "line read must emit `lineam`");
}

#[test]
fn lege_lineam_latin_bare_read_keeps_lege() {
    let code = render(&read_line(false), &LatinSurface);
    assert_eq!(code.trim(), "lege", "bare read must emit `lege`");
}

#[test]
fn lege_lineam_english_line_read_keeps_line() {
    let code = render(&read_line(true), &EnSurface);
    assert_eq!(code.trim(), "line", "en line read must emit `line`");
}

#[test]
fn lege_lineam_english_bare_read_keeps_read() {
    let code = render(&read_line(false), &EnSurface);
    assert_eq!(code.trim(), "read", "en bare read must emit `read`");
}
