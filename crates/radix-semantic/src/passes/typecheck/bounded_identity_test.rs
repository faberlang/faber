//! BB-2 pins: bounded type identity, assignability, char-literal typing, rejects.

use super::*;
use crate::passes::collect;
use crate::passes::resolve::resolve;
use crate::{Resolver, TypeTable};
use radix_hir::HirLiteral;
use radix_lexer::{Interner, Span, lex};
use radix_parser::parse;
use radix_target::Target;

fn intern_cap(types: &mut TypeTable, n: u64) -> crate::IndexId {
    types.intern_index(IndexExpr::Literal(n))
}

#[test]
fn bounded_identity_distinct_from_bare_and_across_n() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let ascii = types.primitive(Primitive::Ascii);
    let textus = types.primitive(Primitive::Textus);
    let n8 = intern_cap(&mut types, 8);
    let n9 = intern_cap(&mut types, 9);
    let n64 = intern_cap(&mut types, 64);
    let n4 = intern_cap(&mut types, 4);

    let ascii8 = types.bounded_ascii(n8);
    let ascii9 = types.bounded_ascii(n9);
    let ascii64 = types.bounded_ascii(n64);
    let textus4 = types.bounded_textus(n4);
    let lista8 = types.bounded_array(numerus, n8);
    let lista = types.array(numerus);

    assert!(!types.equals(ascii8, ascii), "ascii<magnitudo 8> ≠ ascii");
    assert!(
        !types.equals(ascii8, ascii9),
        "ascii<magnitudo 8> ≠ ascii<magnitudo 9>"
    );
    assert!(!types.equals(ascii, ascii64), "ascii ≠ ascii<magnitudo 64>");
    assert!(
        !types.equals(textus4, textus),
        "textus<magnitudo 4> ≠ textus"
    );
    assert!(
        !types.equals(lista8, lista),
        "lista<numerus, magnitudo 8> ≠ lista<numerus>"
    );
    assert!(
        types.is_assignable(ascii8, ascii64),
        "ascii<magnitudo 8> → ascii<magnitudo 64>"
    );
    assert!(
        !types.is_assignable(ascii64, ascii8),
        "ascii<magnitudo 64> ↛ ascii<magnitudo 8>"
    );
    assert!(
        types.is_assignable(ascii8, ascii9),
        "ascii<magnitudo 8> → ascii<magnitudo 9>"
    );
    assert!(!types.is_assignable(ascii8, ascii));
    let n1 = intern_cap(&mut types, 1);
    let ascii1 = types.bounded_ascii(n1);
    assert!(types.is_assignable(ascii1, ascii), "ascii<size 1> → ascii");
    assert!(matches!(types.get(lista), Type::Array(_)));
}

#[test]
fn char_literal_x_is_bounded_ascii_one() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut interner = Interner::new();
    let sym = interner.intern("x");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let ty = checker.literal_type(&HirLiteral::Ascii(sym), Span::default(), None);
    let n1 = intern_cap(checker.types, 1);
    assert!(
        matches!(
            checker.types.get(ty),
            Type::BoundedAscii { capacity } if checker.types.index_equals(*capacity, n1)
        ),
        "'x' is BoundedAscii {{ 1 }}, got {:?}",
        checker.types.get(ty)
    );
}

#[test]
fn multi_byte_ascii_literal_stays_unbounded() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let mut interner = Interner::new();
    let sym = interner.intern("ab");
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let ty = checker.literal_type(&HirLiteral::Ascii(sym), Span::default(), None);
    assert!(
        matches!(checker.types.get(ty), Type::Primitive(Primitive::Ascii)),
        "multi-byte ascii literal stays unbounded, got {:?}",
        checker.types.get(ty)
    );
}

fn resolve_alias(source: &str) -> (TypeTable, crate::TypeId) {
    let mut parsed = parse(lex(source));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    resolve(program, &mut resolver, &parsed.interner, &mut types).expect("resolve");
    let alias = parsed.interner.intern("X");
    let def_id = resolver.lookup(alias).expect("alias def");
    let ty = resolver
        .get_symbol(def_id)
        .and_then(|s| s.ty)
        .expect("alias type");
    (types, ty)
}

#[test]
fn resolve_interns_bounded_forms() {
    let (types, ty) = resolve_alias("typus X = ascii<magnitudo 8>");
    assert!(matches!(
        types.get(ty),
        Type::BoundedAscii { capacity } if matches!(types.get_index(*capacity), IndexExpr::Literal(8))
    ));

    let (types, ty) = resolve_alias("typus X = textus<magnitudo 4>");
    assert!(matches!(
        types.get(ty),
        Type::BoundedTextus { capacity } if matches!(types.get_index(*capacity), IndexExpr::Literal(4))
    ));

    let (types, ty) = resolve_alias("typus X = lista<b8, magnitudo 8>");
    assert!(matches!(
        types.get(ty),
        Type::BoundedOcteti { capacity } if matches!(types.get_index(*capacity), IndexExpr::Literal(8))
    ));

    let (types, ty) = resolve_alias("typus X = lista<numerus, magnitudo 8>");
    assert!(matches!(
        types.get(ty),
        Type::BoundedArray { capacity, .. } if matches!(types.get_index(*capacity), IndexExpr::Literal(8))
    ));

    let (types, ty) = resolve_alias("typus X = lista<numerus>");
    assert!(matches!(types.get(ty), Type::Array(_)));

    let (types, ty) = resolve_alias("typus X = ascii");
    assert!(matches!(types.get(ty), Type::Primitive(Primitive::Ascii)));
}

#[test]
fn magnitudo_n_specializes_bounded_ascii() {
    let mut parsed = parse(lex("typus X<magnitudo N> = ascii<magnitudo N>"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    resolve(program, &mut resolver, &parsed.interner, &mut types).expect("resolve");
    let alias = parsed.interner.intern("X");
    let def_id = resolver.lookup(alias).expect("alias def");
    let ty = resolver
        .get_symbol(def_id)
        .and_then(|s| s.ty)
        .expect("alias type");
    assert!(
        matches!(
            types.get(ty),
            Type::BoundedAscii { capacity } if matches!(types.get_index(*capacity), IndexExpr::Param(_))
        ),
        "ascii<magnitudo N> must intern as BoundedAscii {{ Param(N) }}, got {:?}",
        types.get(ty)
    );
}

#[test]
fn non_natural_n_rejects() {
    let parsed = parse(lex("typus X = ascii<magnitudo numerus>"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    let errors =
        resolve(program, &mut resolver, &parsed.interner, &mut types).expect_err("non-natural N");
    assert!(
        errors.iter().any(|err| err
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "bounded_capacity_not_natural")),
        "expected bounded_capacity_not_natural, got {errors:?}"
    );
}

#[test]
fn unbound_derived_n_plus_one_rejects() {
    // Affine figura arithmetic parses in type-argument position since the
    // K-series parser change (436efde75), so the rejection is semantic: the
    // derived capacity is not a natural and its `N` has no binder.
    let parsed = parse(lex("typus X = ascii<N+1>"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let program = parsed.program.as_ref().expect("program");
    collect::collect(program, &mut resolver, &mut types).expect("collect");
    let errors = resolve(program, &mut resolver, &parsed.interner, &mut types)
        .expect_err("unbound derived N+1");
    let has_issue = |issue: &str| {
        errors.iter().any(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
        })
    };
    assert!(has_issue("bounded_capacity_not_natural"), "{errors:?}");
    assert!(has_issue("figura_unknown_index"), "{errors:?}");
}
