//! Predicate-fold contracts for the Lista-only `omnia`/`quilibet` twins.
//!
//! Both rows reuse the existing `Predicate` parameter shape and publish a
//! boolean result. Runtime empty-list, ordering, and short-circuit laws are
//! exercised by the MIR runner; this module pins semantic admission and the
//! non-boolean callback rejection.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la, entry_local_tys};
use crate::{Primitive, Type};

#[test]
fn predicate_twins_accept_filtrata_shaped_lambdas_and_return_bool() {
    let unit = analyze_ok_la(
        r"incipit {
    sit lista<numerus> xs ← vacua
    sit bivalens all_ok ← xs.omnia(numerus d ∴ d ≥ 0)
    sit bivalens any_negative ← xs.quilibet(numerus d ∴ d ≺ 0)
    nota all_ok
    nota any_negative
}",
    );
    let tys = entry_local_tys(&unit);
    assert_eq!(tys.len(), 3, "one lista and two predicate results");
    for (index, name) in [(1, "omnia"), (2, "quilibet")] {
        assert!(
            matches!(
                unit.types.get(tys[index]),
                Type::Primitive(Primitive::Bivalens)
            ),
            "{name} must return bivalens, got {:?}",
            unit.types.get(tys[index])
        );
    }
}

#[test]
fn predicate_twins_accept_empty_list_contract_without_special_type_path() {
    crate::test_support::assert_accepts_la(
        r"functio check() → bivalens {
    sit lista<numerus> xs ← vacua
    redde xs.omnia(numerus d ∴ d ≥ 0) et non xs.quilibet(numerus d ∴ d ≺ 0)
}",
    );
}

#[test]
fn predicate_twins_reuse_predicate_param_and_reject_non_bool_callback() {
    assert_rejects_with_issue_la(
        r"functio numeric(numerus d) → numerus {
    redde d
}

functio bad(lista<numerus> xs) → bivalens {
    redde xs.omnia(numeric)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn reperta_returns_the_option_of_the_element_type() {
    let unit = analyze_ok_la(
        r"incipit {
    sit lista<numerus> xs ← vacua
    sit numerus ∪ nihil hit ← xs.reperta(numerus d ∴ d ≻ 10)
    nota hit
}",
    );
    let tys = entry_local_tys(&unit);
    assert_eq!(tys.len(), 2, "one lista and one search result");
    let Type::Option(inner) = unit.types.get(tys[1]) else {
        panic!(
            "reperta must return an option, got {:?}",
            unit.types.get(tys[1])
        );
    };
    assert!(matches!(
        unit.types.get(*inner),
        Type::Primitive(Primitive::Numerus)
    ));
}

#[test]
fn reperta_on_a_bounded_list_stays_optional() {
    // A bounded list with a minimum size of one still has no guaranteed match.
    crate::test_support::assert_accepts_la(
        r"functio check(lista<numerus, magnitudo 1…4> xs) → numerus ∪ nihil {
    redde xs.reperta(numerus d ∴ d ≻ 10)
}",
    );
    assert_rejects_with_issue_la(
        r"functio check(lista<numerus, magnitudo 1…4> xs) → numerus {
    redde xs.reperta(numerus d ∴ d ≻ 10)
}",
        "optional_into_plain_cell",
    );
}

#[test]
fn reperta_rejects_a_non_bool_callback_like_the_other_predicate_rows() {
    assert_rejects_with_issue_la(
        r"functio numeric(numerus d) → numerus {
    redde d
}

functio bad(lista<numerus> xs) → numerus ∪ nihil {
    redde xs.reperta(numeric)
}",
        "argument_type_mismatch",
    );
}

#[test]
fn list_reperta_and_regex_inventa_resolve_by_receiver_type() {
    // The regex `inventa` keeps its `match ∪ nihil` result next to the list
    // search; neither method is looked up under the other's receiver.
    crate::test_support::assert_accepts_la(
        r#"incipit {
    sit lista<numerus> xs ← vacua
    sit numerus ∪ nihil hit ← xs.reperta(numerus d ∴ d ≻ 10)
    sit regex re ← "[0-9]+" ↦ regex
    sit _ found ← re.inventa("ab 42")
    nota hit
    nota found
}"#,
    );
    assert_rejects_with_issue_la(
        r#"incipit {
    sit regex re ← "[0-9]+" ↦ regex
    sit _ bad ← re.reperta("ab 42")
    nota bad
}"#,
        "unknown_method",
    );
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<numerus> xs ← vacua
    sit _ bad ← xs.inventa(numerus d ∴ d ≻ 10)
    nota bad
}",
        "unknown_method",
    );
}
