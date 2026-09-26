//! Tests for the target-neutral capability vocabulary.
//!
//! GRAMMAR: pure unit tests over [`CapabilityGap`] and [`Lowerability`]. The
//! LLVM-specific classifier (`classify`) is tested in `llvm_text_test.rs`
//! because it lives under the `llvm_text` module's private surface.

use crate::mir::{CapabilityGap, Lowerability};

#[test]
fn capability_lowerability_first_gap_returns_none_for_capable() {
    assert!(Lowerability::Capable.first_gap().is_none());
    // Even an empty Rejected has no first gap; the API must not panic.
    assert!(Lowerability::Rejected(Vec::new()).first_gap().is_none());
}

#[test]
fn capability_lowerability_first_gap_returns_some_for_rejected() {
    let gap = CapabilityGap::MissingAbiFact { shape: "try_call".to_owned(), slug: "missing-abi-fact" };
    let verdict = Lowerability::Rejected(vec![gap]);
    let first = verdict
        .first_gap()
        .expect("non-empty Rejected has a first gap");
    assert_eq!(first.shape(), "try_call");
    assert_eq!(first.slug(), "missing-abi-fact");
}

#[test]
fn capability_lowerability_is_capable_predicate() {
    assert!(Lowerability::Capable.is_capable());
    assert!(!Lowerability::Rejected(Vec::new()).is_capable());
}

#[test]
fn capability_gap_shape_and_slug_round_trip_across_variants() {
    let cases = [
        (
            CapabilityGap::UnsupportedMirShape { shape: "x".to_owned(), slug: "unsupported-mir-shape" },
            "x",
            "unsupported-mir-shape",
        ),
        (
            CapabilityGap::MissingLayoutFact { shape: "unresolved type".to_owned(), slug: "missing-layout-fact" },
            "unresolved type",
            "missing-layout-fact",
        ),
        (
            CapabilityGap::MissingAbiFact { shape: "try_call".to_owned(), slug: "missing-abi-fact" },
            "try_call",
            "missing-abi-fact",
        ),
        (
            CapabilityGap::MissingRuntimeBoundary { shape: "b".to_owned(), slug: "missing-runtime-boundary" },
            "b",
            "missing-runtime-boundary",
        ),
        (
            CapabilityGap::TargetPolicyRejection { shape: "c".to_owned(), slug: "target-policy-rejection" },
            "c",
            "target-policy-rejection",
        ),
        (
            CapabilityGap::RuntimePanic {
                shape: "runner panic: index out of bounds".to_owned(),
                slug: "runtime-panic",
            },
            "runner panic: index out of bounds",
            "runtime-panic",
        ),
    ];
    for (gap, want_shape, want_slug) in cases {
        assert_eq!(gap.shape(), want_shape);
        assert_eq!(gap.slug(), want_slug);
    }
    assert_eq!(CapabilityGap::UNSUPPORTED_MIR_SHAPE_SLUG, "unsupported-mir-shape");
}
