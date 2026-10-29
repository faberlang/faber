//! BB-7R pins: `lista<T, magnitudo N>` typechecks as `BoundedArray`, D6 appende overflow,
//! and D7 conversions to/from unbounded `lista<T>`.

use crate::test_support::{
    analyze_ok_la, assert_accepts_la, assert_rejects_with_issue_la, entry_local_tys,
};
use crate::{IndexExpr, Type};

#[test]
fn bounded_lista_typechecks_as_bounded_array_distinct_from_unbounded() {
    let unit = analyze_ok_la(
        r"incipit {
    sit lista<numerus, magnitudo 8> bounded ← vacua
    sit lista<numerus> unbounded ← vacua
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
        "lista<numerus, magnitudo 8> must intern as BoundedArray {{ 8 }}, got {:?}",
        unit.types.get(tys[0])
    );
    assert!(
        matches!(unit.types.get(tys[1]), Type::Array(_)),
        "lista<numerus> stays Type::Array, got {:?}",
        unit.types.get(tys[1])
    );
    assert!(
        !unit.types.equals(tys[0], tys[1]),
        "lista<numerus, magnitudo 8> ≠ lista<numerus>"
    );
    assert!(
        !matches!(unit.types.get(tys[1]), Type::BoundedArray { .. }),
        "Type::Array must not grow a capacity field"
    );
}

#[test]
fn bounded_lista_longitudo_and_vacua_are_len_not_capacity() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus, magnitudo 8> xs ← vacua
    sit numerus n ← xs.longitudo()
    sit bivalens empty ← xs.vacua()
    nota n
    nota empty
}",
    );
}

#[test]
fn bounded_lista_accipe_and_index_return_element() {
    assert_accepts_la(
        r"incipit {
    varia lista<numerus, magnitudo 8> xs ← vacua
    sit numerus ∪ nihil got ← xs.accipe(0)
    sit numerus x ← xs[0]
    nota got
    nota x
}",
    );
}

#[test]
fn bounded_lista_literal_indices_reject_provable_oob_and_negative() {
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<numerus, magnitudo 8> xs ← vacua
    sit _ bad ← xs[8]
    nota bad
}",
        "index_provably_out_of_bounds",
    );
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<numerus, magnitudo 8> xs ← vacua
    sit _ bad ← xs[-1]
    nota bad
}",
        "index_provably_out_of_bounds",
    );
}

#[test]
fn bounded_cord_literal_indices_reject_provable_oob_and_negative() {
    for source in [
        r#"incipit {
    sit textus raw ← "a"
    sit textus<magnitudo 8> text ← raw ↦ textus<magnitudo 8>
    sit _ bad ← text[8]
    nota bad
}"#,
        r"incipit {
    sit ascii<magnitudo 8> text ← 'a'
    sit _ bad ← text[-1]
    nota bad
}",
        r"incipit {
    sit lista<b8, magnitudo 8> bytes ← |61| ↦ lista<b8, magnitudo 8>
    sit _ bad ← bytes[8]
    nota bad
}",
    ] {
        assert_rejects_with_issue_la(source, "index_provably_out_of_bounds");
    }
}

#[test]
fn bounded_lista_appende_requires_alternate_exit() {
    // D17.8: `incipit` (main) is exempt; the rule applies in a helper.
    assert_rejects_with_issue_la(
        r"functio grow() → vacuum {
    varia lista<numerus, magnitudo 8> xs ← vacua
    xs.appende(1)
}",
        "failable_call_requires_handler",
    );
}

#[test]
fn bounded_lista_appende_propagates_on_textus_channel() {
    assert_accepts_la(
        r"functio grow() → vacuum ⇥ textus {
    varia lista<numerus, magnitudo 8> xs ← vacua
    xs.appende(1)
}

incipit {
    nota 0
}",
    );
}

#[test]
fn unbounded_lista_appende_stays_infallible() {
    assert_accepts_la(
        r"incipit {
    varia lista<numerus> xs ← vacua
    xs.appende(1)
}",
    );
}

#[test]
fn bounded_lista_sink_requires_alternate_exit() {
    // The sinking method twin of `appende` (the `immerge` row) is the same
    // op, so a bounded receiver reports overflow the same way.
    assert_rejects_with_issue_la(
        r"functio grow() → vacuum {
    varia lista<numerus, magnitudo 8> xs ← vacua
    xs.immerge(1)
}",
        "failable_call_requires_handler",
    );
}

#[test]
fn bounded_lista_sink_propagates_on_textus_channel() {
    assert_accepts_la(
        r"functio grow() → vacuum ⇥ textus {
    varia lista<numerus, magnitudo 8> xs ← vacua
    xs.immerge(1)
}

incipit {
    nota 0
}",
    );
}

#[test]
fn unbounded_lista_sink_stays_infallible() {
    assert_accepts_la(
        r"incipit {
    varia lista<numerus> xs ← vacua
    xs.immerge(1)
}",
    );
}

#[test]
fn bounded_lista_d7_conversions_are_explicit() {
    assert_accepts_la(
        r"incipit {
    sit lista<numerus> unbounded ← [1, 2]
    sit lista<numerus, magnitudo 8> bounded ← unbounded ↦ lista<numerus, magnitudo 8>
    sit lista<numerus> prefix ← bounded ↦ lista<numerus>
    nota prefix
}",
    );
}

#[test]
fn bounded_lista_assignment_is_not_implicit_conversion() {
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<numerus> unbounded ← [1, 2]
    sit lista<numerus, magnitudo 8> bounded ← unbounded
    nota bounded
}",
        "expression_type_mismatch",
    );
}

#[test]
fn unannotated_lista_literal_carries_exact_bound() {
    let unit = analyze_ok_la(
        r"incipit {
    sit _ xs ← [1, 2]
    nota xs
}",
    );
    let ty = entry_local_tys(&unit)[0];
    assert!(
        matches!(
            unit.types.get(ty),
            Type::BoundedArray { capacity, .. }
                if matches!(unit.types.get_index(*capacity), IndexExpr::Literal(2))
        ),
        "unannotated literal must retain exact bound 2, got {:?}",
        unit.types.get(ty)
    );
}

#[test]
fn bounded_lista_hole_solves_from_literal_witness() {
    let unit = analyze_ok_la(
        r"incipit {
    sit lista<numerus, _> xs ← [1, 2]
    nota xs
}",
    );
    let ty = entry_local_tys(&unit)[0];
    assert!(
        matches!(
            unit.types.get(ty),
            Type::BoundedArray { capacity, .. }
                if matches!(unit.types.get_index(*capacity), IndexExpr::Literal(2))
        ),
        "lista<numerus, _> literal witness must solve to 2, got {:?}",
        unit.types.get(ty)
    );
}

#[test]
fn explicit_bounded_lista_literal_uses_capacity_as_its_maximum() {
    assert_accepts_la("incipit { sit lista<numerus, magnitudo 2> xs ← [1, 2] }");
    assert_accepts_la("incipit { sit lista<numerus, magnitudo 3> xs ← [1, 2] }");
    assert_rejects_with_issue_la(
        "incipit { sit lista<numerus, magnitudo 3> xs ← [1, 2, 3, 4] }",
        "incompatible_tensor_index",
    );
}

// Need item 12: an untyped `[]` takes the cell's type, as `vacua`
// does, so a bounded cell keeps its capacity instead of being offered a
// zero-length witness.
#[test]
fn empty_list_literal_takes_the_bounded_cell_capacity() {
    let unit = analyze_ok_la(
        r"incipit {
    varia lista<fractus, magnitudo 4> xs ← []
    nota xs
}",
    );
    let ty = entry_local_tys(&unit)[0];
    assert!(
        matches!(
            unit.types.get(ty),
            Type::BoundedArray { capacity, .. }
                if matches!(unit.types.get_index(*capacity), IndexExpr::Literal(4))
        ),
        "`[]` against lista<fractus, magnitudo 4> must keep capacity 4, got {:?}",
        unit.types.get(ty)
    );
}

#[test]
fn unannotated_literal_pair_length_mismatch_rejects_at_typecheck() {
    assert_rejects_with_issue_la(
        "incipit { sit _ xs ← [1, 2] + [3] }",
        "lista_capacity_mismatch",
    );
}

#[test]
fn bounded_lista_hole_without_literal_witness_still_reports_sem014() {
    crate::test_support::assert_analysis_rejects_with_issue_la(
        "functio missing() → lista<numerus, _> { tacet }",
        "bounded_capacity_unsolved",
    );
}

#[test]
fn bounded_lista_parameter_widening_stays_explicit() {
    // A list value of capacity 2 does not implicitly widen to capacity 3.
    assert_rejects_with_issue_la(
        r"functio consume(lista<numerus, magnitudo 3> xs) → vacuum { nota xs }
incipit {
    sit lista<numerus, magnitudo 2> pair ← [1, 2]
    consume(pair)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn bounded_lista_literal_argument_takes_the_parameter_cell() {
    // A literal is built for its destination cell, as `[]` is: a short literal
    // against a capacity-3 parameter fits it, a longer one is refused by name.
    assert_accepts_la(
        r"functio consume(lista<numerus, magnitudo 3> xs) → vacuum { nota xs }
incipit { consume([1, 2]) }",
    );
    assert_rejects_with_issue_la(
        r"functio consume(lista<numerus, magnitudo 3> xs) → vacuum { nota xs }
incipit { consume([1, 2, 3, 4]) }",
        "argument_type_mismatch",
    );
}
