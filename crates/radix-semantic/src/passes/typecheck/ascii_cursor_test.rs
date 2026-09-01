//! Typecheck pins for `ascii.cursor()` → `cursor<ascii<1>>` (CC-1).

use super::TypeChecker;
use crate::test_support;
use crate::{IndexExpr, Primitive, Resolver, Type, TypeTable};
use radix_intrinsics::{
    IntrinsicReceiver, IntrinsicReturnKind, lookup_method_for_type, receivers_for_type,
};
use radix_lexer::Interner;
use radix_target::Target;

/// Direct table pin: `ascii.cursor()` is `Type::Cursor` of `BoundedAscii { 1 }`.
#[test]
fn ascii_cursor_type_is_cursor_of_bounded_ascii_one() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let ascii = checker.ascii_type();
    let ascii_ty = checker.types.get(ascii).clone();
    assert_eq!(receivers_for_type(&ascii_ty), &[IntrinsicReceiver::Ascii]);
    let entry = lookup_method_for_type(&ascii_ty, "cursor", 0).expect("ascii.cursor");
    assert_eq!(entry.return_kind, IntrinsicReturnKind::CursorAsciiOne);
    let one = checker.ascii_one_type();
    let cursor_ty = checker.types.cursor(one);
    let n1 = checker.types.intern_index(IndexExpr::Literal(1));
    let Type::Cursor(inner) = checker.types.get(cursor_ty).clone() else {
        panic!(
            "ascii.cursor() is Type::Cursor, got {:?}",
            checker.types.get(cursor_ty)
        );
    };
    assert!(
        matches!(
            checker.types.get(inner),
            Type::BoundedAscii { capacity } if checker.types.index_equals(*capacity, n1)
        ),
        "ascii.cursor() element is BoundedAscii {{ 1 }}, got {:?}",
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
fn ascii_cursor_accepts_as_cursor_ascii_one() {
    test_support::assert_accepts(
        r"functio walk(ascii s) → cursor<ascii<1>> {
    redde s.cursor()
}",
    );
}

#[test]
fn itera_ex_ascii_cursor_binds_ascii_one() {
    test_support::assert_accepts(
        r"incipit {
    fixum ascii s ← 'ab'
    itera ex s.cursor() fixum c {
        fixum ascii<1> x ← c
        nota x
    }
}",
    );
}

#[test]
fn ascii_cursor_collects_to_lista_ascii_one() {
    test_support::assert_accepts(
        r"incipit {
    fixum ascii s ← 'ab'
    fixum lista<ascii<1>> xs ← s.cursor() ↦ lista<ascii<1>>
    nota xs
}",
    );
}

#[test]
fn ascii_cursor_collect_to_lista_textus_rejects() {
    test_support::assert_rejects_with_issue(
        r"incipit {
    fixum ascii s ← 'ab'
    fixum lista<textus> xs ← s.cursor() ↦ lista<textus>
    nota xs
}",
        "cross_collection_unsupported",
    );
}

#[test]
fn itera_de_ascii_cursor_requires_ex() {
    test_support::assert_rejects_with_issue(
        r"incipit {
    fixum ascii s ← 'ab'
    itera de s.cursor() fixum c {
        nota c
    }
}",
        "cursor_iteration_requires_ex",
    );
}
