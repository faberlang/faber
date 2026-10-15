//! Typecheck pins for `ascii.cursor()` → `cursor<littera>` (CC-1).

use super::TypeChecker;
use crate::test_support;
use crate::{Primitive, Resolver, Type, TypeTable};
use radix_intrinsics::{
    IntrinsicReceiver, IntrinsicReturnKind, lookup_method_for_type, receivers_for_type,
};
use radix_lexer::Interner;
use radix_target::Target;

/// Direct table pin: `ascii.cursor()` is `Type::Cursor` of `littera` (D10.1).
#[test]
fn ascii_cursor_type_is_cursor_of_littera() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let ascii = checker.ascii_type();
    let ascii_ty = checker.types.get(ascii).clone();
    assert_eq!(receivers_for_type(&ascii_ty), &[IntrinsicReceiver::Ascii]);
    let entry = lookup_method_for_type(&ascii_ty, "cursor", 0).expect("ascii.cursor");
    assert_eq!(entry.return_kind, IntrinsicReturnKind::CursorLittera);
    let element = checker.text_element_type();
    let cursor_ty = checker.types.cursor(element);
    let Type::Cursor(inner) = checker.types.get(cursor_ty).clone() else {
        panic!(
            "ascii.cursor() is Type::Cursor, got {:?}",
            checker.types.get(cursor_ty)
        );
    };
    assert!(
        matches!(
            checker.types.get(inner),
            Type::Primitive(Primitive::Littera)
        ),
        "ascii.cursor() element is littera (D10.1), got {:?}",
        checker.types.get(inner)
    );
}

#[test]
fn ascii_cursor_accepts_as_cursor_littera() {
    test_support::assert_accepts_la(
        r"functio walk(ascii s) → cursor<littera> {
    redde s.cursor()
}",
    );
}

#[test]
fn itera_ex_ascii_cursor_binds_littera() {
    test_support::assert_accepts_la(
        r"incipit {
    sit ascii s ← 'ab'
    itera ex s.cursor() fixum c {
        sit littera x ← c
        nota x
    }
}",
    );
}

#[test]
fn ascii_cursor_collects_to_lista_littera() {
    test_support::assert_accepts_la(
        r"incipit {
    sit ascii s ← 'ab'
    sit lista<littera> xs ← s.cursor() ↦ lista<littera>
    nota xs
}",
    );
}

#[test]
fn ascii_cursor_collect_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
    sit ascii s ← 'ab'
    sit lista<textus> xs ← s.cursor() ↦ lista<textus>
    nota xs
}",
        "cross_collection_unsupported",
    );
}

#[test]
fn itera_de_ascii_cursor_requires_ex() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
    sit ascii s ← 'ab'
    itera de s.cursor() fixum c {
        nota c
    }
}",
        "cursor_iteration_requires_ex",
    );
}
