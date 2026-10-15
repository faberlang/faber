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
