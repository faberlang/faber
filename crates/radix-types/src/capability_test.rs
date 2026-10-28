use super::*;

fn rejected<P>(answer: &CapabilityAnswer<P>, reason: CapabilityReason) {
    assert_eq!(answer.reason(), Some(reason));
    assert!(answer.proof().is_none());
}

#[test]
fn vocabulary_keeps_similar_type_capabilities_distinct() {
    assert_ne!(
        TypeCapabilityKind::ExplicitlyDuplicable,
        TypeCapabilityKind::TriviallyDuplicable
    );
    assert_ne!(
        TypeCapabilityKind::StrictEquality,
        TypeCapabilityKind::ReflexiveEquality
    );
    assert_ne!(
        TypeCapabilityKind::StrictEquality,
        TypeCapabilityKind::CanonicalHashEligibility
    );

    assert_ne!(
        TypeCapabilityProof::ExplicitlyDuplicable,
        TypeCapabilityProof::TriviallyDuplicable
    );
    assert_ne!(
        TypeCapabilityProof::StrictlyEqual,
        TypeCapabilityProof::ReflexivelyEqual
    );
    assert_ne!(
        TypeCapabilityProof::StrictlyEqual,
        TypeCapabilityProof::CanonicalHashEligible
    );
}

#[test]
fn every_classified_reason_has_a_stable_code() {
    for reason in CapabilityReason::ALL {
        assert_ne!(reason.code(), "");
        assert_eq!(reason.to_string(), reason.code());
    }
}

#[test]
fn recursion_has_explicit_intermediate_and_terminal_outcomes() {
    let visiting: RecursionOutcome<TypeCapabilityProof> = RecursionOutcome::Visiting;
    let proven = RecursionOutcome::Proven(TypeCapabilityProof::Transferable);
    let unsupported: RecursionOutcome<TypeCapabilityProof> =
        RecursionOutcome::Unsupported(CapabilityReason::UnsupportedRecursion);

    assert!(matches!(visiting, RecursionOutcome::Visiting));
    assert!(matches!(proven, RecursionOutcome::Proven(_)));
    assert!(matches!(unsupported, RecursionOutcome::Unsupported(_)));

    assert_eq!(
        visiting.into_answer().reason(),
        Some(CapabilityReason::UnsupportedRecursion)
    );
    assert!(matches!(
        proven.into_answer(),
        CapabilityAnswer::Proven(TypeCapabilityProof::Transferable)
    ));
    assert_eq!(
        unsupported.into_answer().reason(),
        Some(CapabilityReason::UnsupportedRecursion)
    );
}

#[test]
fn missing_type_summary_is_fail_closed_for_all_type_rows() {
    let summary = TypeCapabilitySummary::missing(TypeId(7));
    assert_eq!(summary.type_id, TypeId(7));

    for kind in TypeCapabilityKind::ALL {
        rejected(summary.capability(kind), CapabilityReason::MissingFact);
    }
}

#[test]
fn summaries_retain_existing_identity_keys() {
    let index = IndexCapabilitySummary::missing(IndexId(3));
    let function = FunctionCapabilitySummary::missing(DefId(11));

    assert_eq!(index.index_id, IndexId(3));
    rejected(&index.solved_identity, CapabilityReason::MissingFact);
    assert_eq!(function.def_id, DefId(11));

    for kind in FunctionCapabilityKind::ALL {
        rejected(function.capability(kind), CapabilityReason::MissingFact);
    }
}

#[test]
fn indexed_identity_proof_preserves_ordered_existing_index_ids() {
    let proof = SolvedIndexedIdentity {
        width: Some(IndexId(1)),
        capacity: Some(IndexId(2)),
        rank: Some(IndexId(3)),
        dimensions: vec![IndexId(4), IndexId(5)],
        tuple_order: vec![IndexId(6), IndexId(7)],
        size_arguments: vec![IndexId(8), IndexId(9)],
    };

    assert_eq!(proof.dimensions, [IndexId(4), IndexId(5)]);
    assert_eq!(proof.tuple_order, [IndexId(6), IndexId(7)]);
    assert_eq!(proof.size_arguments, [IndexId(8), IndexId(9)]);
}
