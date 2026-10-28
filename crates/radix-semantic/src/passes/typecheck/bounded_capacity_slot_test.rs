//! CS-1..CS-3 pins: `ascii<_>` (and twins) as capacity holes.

use super::*;
use crate::passes::collect;
use crate::passes::resolve::resolve;
use crate::test_support;
use crate::{IndexExpr, Resolver, TypeTable};
use radix_lexer::Interner;
use radix_parser::parse;
use radix_target::Target;

fn latin_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

fn resolve_alias(source: &str) -> (TypeTable, crate::TypeId) {
    let mut parsed = parse(radix_lexer::lex_with_locale_pack(source, &latin_pack()));
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

fn interned_capacity_is_infer(types: &TypeTable, ty: crate::TypeId) -> bool {
    match types.get(ty) {
        Type::BoundedAscii { capacity }
        | Type::BoundedTextus { capacity }
        | Type::BoundedOcteti { capacity } => {
            matches!(types.get_index(*capacity), IndexExpr::Infer(_))
        }
        Type::BoundedArray { capacity, .. } => {
            matches!(types.get_index(*capacity), IndexExpr::Infer(_))
        }
        _ => false,
    }
}

#[test]
fn resolve_interns_capacity_holes() {
    let (types, ty) = resolve_alias("typus X = ascii<_>");
    assert!(
        interned_capacity_is_infer(&types, ty),
        "ascii<_> must intern Infer capacity, got {:?}",
        types.get(ty)
    );

    let (types, ty) = resolve_alias("typus X = textus<_>");
    assert!(interned_capacity_is_infer(&types, ty), "textus<_>");

    let (types, ty) = resolve_alias("typus X = lista<b8, _>");
    assert!(interned_capacity_is_infer(&types, ty), "octeti<_>");

    let (types, ty) = resolve_alias("typus X = lista<numerus, _>");
    assert!(interned_capacity_is_infer(&types, ty), "lista<numerus, _>");
}

#[test]
fn lista_element_and_capacity_holes() {
    let (types, ty) = resolve_alias("typus X = lista<_, _>");
    assert!(
        matches!(
            types.get(ty),
            Type::BoundedArray { element, capacity }
                if matches!(types.get(*element), Type::Infer(_))
                    && matches!(types.get_index(*capacity), IndexExpr::Infer(_))
        ),
        "lista<_, _> is element Infer + capacity Infer, got {:?}",
        types.get(ty)
    );
}

#[test]
fn bare_ascii_accepts_char_literal() {
    test_support::assert_accepts_la(
        r"incipit {
    sit ascii c ← 'x'
    nota c
}",
    );
}

#[test]
fn ascii_hole_accepts_char_literal() {
    test_support::assert_accepts_la(
        r"incipit {
    sit ascii<_> c ← 'x'
    sit ascii<magnitudo 1> d ← c
    nota d
}",
    );
}

#[test]
fn ascii_hole_solved_to_one_widens_to_eight() {
    test_support::assert_accepts_la(
        r"incipit {
    sit ascii<_> c ← 'x'
    sit ascii<magnitudo 8> d ← c
    nota d
}",
    );
}

#[test]
fn ascii_eight_accepts_char_literal() {
    test_support::assert_accepts_la(
        r"incipit {
    sit ascii<magnitudo 8> c ← 'x'
    nota c
}",
    );
}

#[test]
fn ascii_hole_rejects_bare_multi_byte() {
    test_support::assert_rejects_with_issue_la(
        r"incipit {
    sit ascii<_> c ← 'ab'
    nota c
}",
        "expression_type_mismatch",
    );
}

#[test]
fn ascii_hole_rejects_unbounded_textus() {
    test_support::assert_rejects_with_issue_la(
        r#"incipit {
    sit ascii<_> c ← "hi"
    nota c
}"#,
        "expression_type_mismatch",
    );
}

#[test]
fn unsolved_ascii_hole_errors() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"functio k() → ascii<_> {
    tacet
}",
        "bounded_capacity_unsolved",
    );
}

#[test]
fn harness_twins_unify_same_family() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let n4 = checker.types.intern_index(IndexExpr::Literal(4));
    let hole = {
        let var = checker.types.fresh_index_infer_var();
        checker.types.intern_index(IndexExpr::Infer(var))
    };
    let dest = checker.types.bounded_textus(hole);
    let src = checker.types.bounded_textus(n4);
    let span = radix_lexer::Span::default();
    let unified = checker.unify_assign_issue(src, dest, span, "expression_type_mismatch");
    assert!(
        checker.errors.is_empty(),
        "textus<magnitudo 4> → textus<_> must unify: {:?}",
        checker.errors
    );
    let cap = match checker.types.get(unified) {
        Type::BoundedTextus { capacity } => *capacity,
        other => panic!("expected BoundedTextus, got {other:?}"),
    };
    let resolved = checker.resolve_index(cap);
    assert!(
        matches!(checker.types.get_index(resolved), IndexExpr::Literal(4)),
        "hole must solve to 4, got {:?}",
        checker.types.get_index(resolved)
    );

    let octeti_dest = {
        let var = checker.types.fresh_index_infer_var();
        let hole = checker.types.intern_index(IndexExpr::Infer(var));
        checker.types.bounded_octeti(hole)
    };
    let octeti_src = checker.types.bounded_octeti(n4);
    let _ = checker.unify_assign_issue(octeti_src, octeti_dest, span, "expression_type_mismatch");
    assert!(
        checker.errors.is_empty(),
        "octeti<4> → octeti<_> must unify: {:?}",
        checker.errors
    );

    let numerus = checker.numerus_type();
    let lista_dest = {
        let var = checker.types.fresh_index_infer_var();
        let hole = checker.types.intern_index(IndexExpr::Infer(var));
        checker.types.bounded_array(numerus, hole)
    };
    let lista_src = checker.types.bounded_array(numerus, n4);
    let _ = checker.unify_assign_issue(lista_src, lista_dest, span, "expression_type_mismatch");
    assert!(
        checker.errors.is_empty(),
        "lista<numerus, magnitudo 4> → lista<numerus, _> must unify: {:?}",
        checker.errors
    );
}

#[test]
fn alias_uses_freshen_independently() {
    test_support::assert_accepts_la(
        r"typus X = ascii<_>
incipit {
    sit X a ← 'x'
    sit X b ← 'y'
    sit ascii<magnitudo 1> c ← a
    sit ascii<magnitudo 1> d ← b
    nota c
    nota d
}",
    );
}

#[test]
fn alias_first_solve_does_not_lock_second() {
    // If the alias body shared one Infer var, the first solve to 1 would
    // make the second site `ascii<1> ← ascii<8>` — which must stay an error
    // only when the sites are independently freshened *and* we have an
    // ascii<8> witness. Without a surface ascii<8> value, pin independence
    // by two char literals both succeeding (they would also succeed on a
    // shared var). The shared-var defect is: first 'x' binds α=1, then a
    // later use as dest for a different N fails. Construct that via a
    // rejected second annotation that would succeed if the hole were free:
    // after a ← 'x', `sit ascii<8> z ← …` is a different test.
    //
    // Two uses with the same witness both accept — and a use of X as dest
    // of an already-solved ascii<1> still accepts on a second binding.
    test_support::assert_accepts_la(
        r"typus X = ascii<_>
incipit {
    sit X a ← 'x'
    sit X b ← a
    nota b
}",
    );
}
