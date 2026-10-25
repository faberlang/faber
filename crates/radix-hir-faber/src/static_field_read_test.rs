//! A static (`generis`) field read through the type name re-emits as the
//! source spelling `Tab.Z`; `ego` (which lowers to the same `Path(genus)`)
//! keeps its keyword for instance fields.

use crate::surface::{KeywordSurface, TypeBindingKind};
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{HirExpression, HirExpressionKind};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{DefId, TypeTable, USER_DEF_ID_BASE};

struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "ego"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: radix_hir::HirId(1),
        kind,
        ty: None,
        span: Span::new(0, 0),
    }
}

/// Render `<Path(Tab)>.<field>` with `Tab` a local genus declaring `Z` static.
fn render_read(field: &str, static_field: Option<&str>) -> String {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let tab = DefId(USER_DEF_ID_BASE);
    let mut names: FxHashMap<DefId, Symbol> = FxHashMap::default();
    names.insert(tab, interner.intern("Tab"));
    let field_sym = interner.intern(field);
    let static_sym = static_field.map(|name| interner.intern(name));
    let mut codegen = crate::FaberCodegen::new(&types, &interner);
    codegen.self_def_ids.insert(tab);
    if let Some(sym) = static_sym {
        codegen.static_fields.insert((tab, sym));
    }
    let read = expr(HirExpressionKind::Field(
        Box::new(expr(HirExpressionKind::Path(tab))),
        field_sym,
    ));
    let mut writer = CodeWriter::new();
    codegen.write_expr(&read, &names, &TestSurface, &mut writer);
    writer.finish()
}

#[test]
fn static_field_read_re_emits_the_type_name() {
    assert_eq!(render_read("Z", Some("Z")), "Tab.Z");
}

#[test]
fn static_field_read_leaves_instance_field_on_ego() {
    assert_eq!(render_read("w", Some("Z")), "ego.w");
}
