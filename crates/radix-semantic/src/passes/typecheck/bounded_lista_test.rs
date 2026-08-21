//! BB-7R pins: `lista<T, N>` typechecks as `BoundedArray`, D6 appende overflow,
//! and D7 conversions to/from unbounded `lista<T>`.

use crate::test_support::{analyze_ok, assert_accepts, assert_rejects_with_issue, entry_local_tys};
use crate::{IndexExpr, Type};

#[test]
fn bounded_lista_typechecks_as_bounded_array_distinct_from_unbounded() {
    let unit = analyze_ok(
        r"incipit {
    fixum lista<numerus, 8> bounded ← vacua
    fixum lista<numerus> unbounded ← vacua
    nota bounded
    nota unbounded
}",
    );
    let tys = entry_local_tys(&unit);
    assert_eq!(tys.len(), 2);
    assert!(
        matches!(
            unit.types.get(tys[0]),
            Type::BoundedArray { capacity, .. }
                if matches!(unit.types.get_index(*capacity), IndexExpr::Literal(8))
        ),
        "lista<numerus, 8> must intern as BoundedArray {{ 8 }}, got {:?}",
        unit.types.get(tys[0])
    );
    assert!(
        matches!(unit.types.get(tys[1]), Type::Array(_)),
        "lista<numerus> stays Type::Array, got {:?}",
        unit.types.get(tys[1])
    );
    assert!(
        !unit.types.equals(tys[0], tys[1]),
        "lista<numerus, 8> ≠ lista<numerus>"
    );
    assert!(
        !matches!(unit.types.get(tys[1]), Type::BoundedArray { .. }),
        "Type::Array must not grow a capacity field"
    );
}

#[test]
fn bounded_lista_longitudo_and_vacua_are_len_not_capacity() {
    assert_accepts(
        r"incipit {
    fixum lista<numerus, 8> xs ← vacua
    fixum numerus n ← xs.longitudo()
    fixum bivalens empty ← xs.vacua()
    nota n
    nota empty
}",
    );
}

#[test]
fn bounded_lista_accipe_and_index_return_element() {
    assert_accepts(
        r"incipit {
    varia lista<numerus, 8> xs ← vacua
    fixum numerus ∪ nihil got ← xs.accipe(0)
    fixum numerus x ← xs[0]
    nota got
    nota x
}",
    );
}

#[test]
fn bounded_lista_appende_requires_alternate_exit() {
    assert_rejects_with_issue(
        r"incipit {
    varia lista<numerus, 8> xs ← vacua
    xs.appende(1)
}",
        "failable_call_requires_handler",
    );
}

#[test]
fn bounded_lista_appende_propagates_on_textus_channel() {
    assert_accepts(
        r"functio grow() → vacuum ⇥ textus {
    varia lista<numerus, 8> xs ← vacua
    xs.appende(1)
}

incipit {
    nota 0
}",
    );
}

#[test]
fn unbounded_lista_appende_stays_infallible() {
    assert_accepts(
        r"incipit {
    varia lista<numerus> xs ← vacua
    xs.appende(1)
}",
    );
}

#[test]
fn bounded_lista_d7_conversions_are_explicit() {
    assert_accepts(
        r"incipit {
    fixum lista<numerus> unbounded ← [1, 2]
    fixum lista<numerus, 8> bounded ← unbounded ↦ lista<numerus, 8>
    fixum lista<numerus> prefix ← bounded ↦ lista<numerus>
    nota prefix
}",
    );
}

#[test]
fn bounded_lista_assignment_is_not_implicit_conversion() {
    assert_rejects_with_issue(
        r"incipit {
    fixum lista<numerus> unbounded ← [1, 2]
    fixum lista<numerus, 8> bounded ← unbounded
    nota bounded
}",
        "expression_type_mismatch",
    );
}
