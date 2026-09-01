//! Typecheck pins for `textus.cursor()` / en `string.cursor()` → `cursor<ascii<1>>` (CC-2).

use super::TypeChecker;
use crate::locale::LocalePack;
use crate::test_support;
use crate::{IndexExpr, Primitive, Resolver, Type, TypeTable};
use radix_intrinsics::{
    IntrinsicReceiver, IntrinsicReturnKind, lookup_method_for_type, receivers_for_type,
};
use radix_lexer::Interner;
use radix_target::Target;
use std::path::PathBuf;

/// Direct table pin: `textus.cursor()` is the same `ascii_one_type()` as `textus[i]`.
#[test]
fn textus_cursor_type_is_cursor_of_bounded_ascii_one() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let textus = checker.textus_type();
    let textus_ty = checker.types.get(textus).clone();
    assert_eq!(receivers_for_type(&textus_ty), &[IntrinsicReceiver::Textus]);
    let entry = lookup_method_for_type(&textus_ty, "cursor", 0).expect("textus.cursor");
    assert_eq!(entry.return_kind, IntrinsicReturnKind::CursorAsciiOne);
    let one = checker.ascii_one_type();
    let cursor_ty = checker.types.cursor(one);
    let n1 = checker.types.intern_index(IndexExpr::Literal(1));
    let Type::Cursor(inner) = checker.types.get(cursor_ty).clone() else {
        panic!(
            "textus.cursor() is Type::Cursor, got {:?}",
            checker.types.get(cursor_ty)
        );
    };
    assert!(
        matches!(
            checker.types.get(inner),
            Type::BoundedAscii { capacity } if checker.types.index_equals(*capacity, n1)
        ),
        "textus.cursor() element is BoundedAscii {{ 1 }}, got {:?}",
        checker.types.get(inner)
    );
    assert!(
        !matches!(
            checker.types.get(inner),
            Type::Primitive(Primitive::Ascii | Primitive::Textus | Primitive::Numerus)
        ),
        "cursor element must not be bare ascii / textus / numerus"
    );
}

#[test]
fn textus_cursor_accepts_as_cursor_ascii_one() {
    test_support::assert_accepts(
        r"functio walk(textus s) → cursor<ascii<1>> {
    redde s.cursor()
}",
    );
}

#[test]
fn itera_ex_textus_cursor_binds_ascii_one() {
    test_support::assert_accepts(
        r#"incipit {
    fixum textus s ← "ab"
    itera ex s.cursor() fixum c {
        fixum ascii<1> x ← c
        nota x
    }
}"#,
    );
}

#[test]
fn textus_cursor_collects_to_lista_ascii_one() {
    test_support::assert_accepts(
        r#"incipit {
    fixum textus s ← "ab"
    fixum lista<ascii<1>> xs ← s.cursor() ↦ lista<ascii<1>>
    nota xs
}"#,
    );
}

#[test]
fn quoted_textus_cursor_collects_to_lista_ascii_one() {
    test_support::assert_accepts(
        r#"incipit {
    fixum lista<ascii<1>> xs ← "ab".cursor() ↦ lista<ascii<1>>
    nota xs
}"#,
    );
}

#[test]
fn textus_cursor_collect_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue(
        r#"incipit {
    fixum textus s ← "ab"
    fixum lista<textus> xs ← s.cursor() ↦ lista<textus>
    nota xs
}"#,
        "cross_collection_unsupported",
    );
}

#[test]
fn itera_de_textus_cursor_requires_ex() {
    test_support::assert_rejects_with_issue(
        r#"incipit {
    fixum textus s ← "ab"
    itera de s.cursor() fixum c {
        nota c
    }
}"#,
        "cursor_iteration_requires_ex",
    );
}

#[test]
fn non_ascii_textus_cursor_still_types_as_cursor_ascii_one() {
    test_support::assert_accepts(
        r#"functio walk(textus s) → cursor<ascii<1>> {
    redde "é".cursor()
}"#,
    );
}

fn en_pack() -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    LocalePack::from_toml_path(&path).expect("installed en pack should load")
}

#[test]
fn en_string_cursor_accepts_as_iterator_ascii_one() {
    let source = r"fn walk(string s) → iterator<ascii<1>> {
    return s.cursor()
}";
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}
