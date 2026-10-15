//! LSR-W0-SEMANTIC typecheck pins for the argmaxima/argminima method twins:
//! the `numerus ∪ nihil` index shape on numeric lista receivers, the `vel`
//! composition that shape buys, and the numeric-element gate. The
//! tie/NaN/empty execution laws are pinned by the owning runner tests
//! (`radix-mir-runner/src/arg_extremum_test.rs`).

use crate::Type;
use crate::test_support::{
    analyze_ok_la, assert_accepts_la, assert_rejects_with_issue_la, entry_local_tys,
};

#[test]
fn argmaxima_resolves_on_numeric_lista_as_numerus_option() {
    let unit = analyze_ok_la(
        r"incipit {
    sit lista<fractus> xs ← vacua
    sit numerus ∪ nihil winner ← xs.argmaxima()
    nota winner
}",
    );
    let tys = entry_local_tys(&unit);
    assert_eq!(tys.len(), 2, "one lista local and one winner local");
    assert!(
        matches!(unit.types.get(tys[1]), Type::Option(inner)
            if matches!(unit.types.get(*inner), Type::Primitive(crate::Primitive::Numerus))),
        "argmaxima must return numerus ∪ nihil, got {:?}",
        unit.types.get(tys[1])
    );
}

#[test]
fn argmaxima_empty_result_composes_with_vel() {
    assert_accepts_la(
        r"incipit {
    sit lista<fractus> xs ← vacua
    sit numerus literal ← xs.argmaxima() vel 0
    sit numerus fallback ← 0
    sit numerus from_var ← xs.argmaxima() vel fallback
    sit numerus mirrored ← xs.argminima() vel 0
    nota literal
    nota from_var
    nota mirrored
}",
    );
}

#[test]
fn argminima_mirrors_argmaxima_on_numeric_lista() {
    let unit = analyze_ok_la(
        r"incipit {
    sit lista<fractus> xs ← vacua
    sit numerus ∪ nihil winner ← xs.argminima()
    nota winner
}",
    );
    let tys = entry_local_tys(&unit);
    assert!(
        matches!(unit.types.get(tys[1]), Type::Option(inner)
            if matches!(unit.types.get(*inner), Type::Primitive(crate::Primitive::Numerus))),
        "argminima must mirror argmaxima's numerus ∪ nihil shape, got {:?}",
        unit.types.get(tys[1])
    );
}

#[test]
fn argmaxima_rejects_non_numeric_lista_elements() {
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<textus> xs ← vacua
    sit numerus ∪ nihil winner ← xs.argmaxima()
    nota winner
}",
        "numeric_lista_elements_required",
    );
    assert_rejects_with_issue_la(
        r"incipit {
    sit lista<textus> xs ← vacua
    sit numerus ∪ nihil winner ← xs.argminima()
    nota winner
}",
        "numeric_lista_elements_required",
    );
}
