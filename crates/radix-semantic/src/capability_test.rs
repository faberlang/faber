use super::*;
use radix_hir::HirId;

fn assert_missing<P>(answer: &CapabilityAnswer<P>) {
    assert_eq!(answer.reason(), Some(CapabilityReason::MissingFact));
    assert!(answer.proof().is_none());
}

#[test]
fn fail_closed_provider_rejects_every_type_and_index_query() {
    let provider = FailClosedCapabilityQueries;
    let type_id = TypeId(1);
    let index_id = IndexId(2);

    for kind in TypeCapabilityKind::ALL {
        assert_missing(&provider.type_capability(type_id, kind));
    }

    for kind in IndexCapabilityKind::ALL {
        assert_missing(&provider.index_capability(index_id, kind));
    }
}

#[test]
fn fail_closed_provider_rejects_every_place_query() {
    let provider = FailClosedCapabilityQueries;
    let hir_id = HirId(3);

    for kind in PlaceCapabilityKind::ALL {
        assert_missing(&provider.place_capability(hir_id, kind));
    }
}

#[test]
fn fail_closed_provider_rejects_every_function_query() {
    let provider = FailClosedCapabilityQueries;
    let def_id = DefId(4);

    for kind in FunctionCapabilityKind::ALL {
        assert_missing(&provider.function_capability(def_id, kind));
    }
}

#[test]
fn place_summary_is_keyed_by_hir_identity_and_fail_closed() {
    let summary = PlaceCapabilitySummary::missing(HirId(9));
    assert_eq!(summary.hir_id, HirId(9));

    for kind in PlaceCapabilityKind::ALL {
        assert_missing(summary.capability(kind));
    }
}

#[test]
fn every_classified_reason_remains_an_answer_not_a_negative_proof() {
    for reason in CapabilityReason::ALL {
        let answer: CapabilityAnswer<()> = CapabilityAnswer::Rejected(reason);
        assert_eq!(answer.reason(), Some(reason));
        assert!(answer.proof().is_none());
    }
}

#[test]
fn combined_query_interface_is_implemented_by_fail_closed_provider() {
    fn accepts_combined_provider<T: CapabilityQueries>(_: &T) {}

    accepts_combined_provider(&FailClosedCapabilityQueries);
    fn accepts_singular_provider<T: CapabilityQuery>(_: &T) {}
    accepts_singular_provider(&FailClosedCapabilityQueries);
}
