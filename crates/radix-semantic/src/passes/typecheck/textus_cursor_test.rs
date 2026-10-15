//! Typecheck pins for `textus.cursor()` / en `string.cursor()` → `cursor<littera>` (CC-2).

use super::TypeChecker;
use crate::locale::LocalePack;
use crate::test_support;
use crate::{Primitive, Resolver, Type, TypeTable};
use radix_intrinsics::{
    IntrinsicReceiver, IntrinsicReturnKind, lookup_method_for_type, receivers_for_type,
};
use radix_lexer::Interner;
use radix_target::Target;

/// Direct table pin: `textus.cursor()` yields the `littera` text element (D10.1).
#[test]
fn textus_cursor_type_is_cursor_of_littera() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let textus = checker.textus_type();
    let textus_ty = checker.types.get(textus).clone();
    assert_eq!(receivers_for_type(&textus_ty), &[IntrinsicReceiver::Textus]);
    let entry = lookup_method_for_type(&textus_ty, "cursor", 0).expect("textus.cursor");
    assert_eq!(entry.return_kind, IntrinsicReturnKind::CursorLittera);
    let element = checker.text_element_type();
    let cursor_ty = checker.types.cursor(element);
    let Type::Cursor(inner) = checker.types.get(cursor_ty).clone() else {
        panic!(
            "textus.cursor() is Type::Cursor, got {:?}",
            checker.types.get(cursor_ty)
        );
    };
    assert!(
        matches!(
            checker.types.get(inner),
            Type::Primitive(Primitive::Littera)
        ),
        "textus.cursor() element is littera (D10.1), got {:?}",
        checker.types.get(inner)
    );
}

#[test]
fn textus_cursor_accepts_as_cursor_littera() {
    test_support::assert_accepts(
        r"functio walk(textus s) → cursor<littera> {
    redde s.cursor()
}",
    );
}

#[test]
fn itera_ex_textus_cursor_binds_littera() {
    test_support::assert_accepts(
        r#"incipit {
    sit textus s ← "ab"
    itera ex s.cursor() fixum c {
        sit littera x ← c
        nota x
    }
}"#,
    );
}

#[test]
fn textus_cursor_collects_to_lista_littera() {
    test_support::assert_accepts(
        r#"incipit {
    sit textus s ← "ab"
    sit lista<littera> xs ← s.cursor() ↦ lista<littera>
    nota xs
}"#,
    );
}

#[test]
fn quoted_textus_cursor_collects_to_lista_littera() {
    test_support::assert_accepts(
        r#"incipit {
    sit lista<littera> xs ← "ab".cursor() ↦ lista<littera>
    nota xs
}"#,
    );
}

#[test]
fn textus_cursor_collect_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue(
        r#"incipit {
    sit textus s ← "ab"
    sit lista<textus> xs ← s.cursor() ↦ lista<textus>
    nota xs
}"#,
        "cross_collection_unsupported",
    );
}

#[test]
fn itera_de_textus_cursor_requires_ex() {
    test_support::assert_rejects_with_issue(
        r#"incipit {
    sit textus s ← "ab"
    itera de s.cursor() fixum c {
        nota c
    }
}"#,
        "cursor_iteration_requires_ex",
    );
}

#[test]
fn non_ascii_textus_cursor_types_as_cursor_littera() {
    test_support::assert_accepts(
        r#"functio walk(textus s) → cursor<littera> {
    redde "é".cursor()
}"#,
    );
}

fn en_pack() -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path("en");
    LocalePack::from_toml_path(&path).expect("installed en pack should load")
}

#[test]
fn en_string_cursor_accepts_as_iterator_littera() {
    let source = r"fn walk(string s) → iterator<char> {
    return s.cursor()
}";
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}
