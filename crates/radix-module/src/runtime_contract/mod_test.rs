//! Runtime contract equivalence checks.
//!
//! These pin the Radix-owned copies against known public `faber` crate values and
//! datetime round-trips, so a transcription error in the copied contract is
//! caught here rather than as silent emit or behavior drift.

use crate::runtime_contract::{
    display::{display_bivalens, display_fractus},
    display_tokens::DisplayTokens,
    frame::FrameStatus,
    instans::{Instans, InstansPraecisio},
    sparsa, tensor,
};

#[test]
fn tensor_error_constants_match_runtime_text() {
    // Spot-check the constants actually consumed by codegen/runner; these must
    // be byte-identical to the public faber crate so compile-time and runtime agree.
    assert_eq!(tensor::ERR_CREA_INVALID_SHAPE, "tensor crea invalid shape");
    assert_eq!(tensor::ERR_FORMA_RESHAPE_COUNT, "tensor forma (reshape) element count mismatch");
    assert_eq!(tensor::ERR_FORMA_ELEMENT_COUNT, "tensor forma element count mismatch");
    assert_eq!(tensor::ERR_BROADCAST_SHAPE, "tensor broadcast shape mismatch");
    assert_eq!(tensor::ERR_SECTIO_INVALID_SLICE_BOUNDS, "tensor sectio invalid slice bounds");
    assert_eq!(tensor::ERR_ACCIPE_INVALID_INDEX, "tensor accipe invalid index");
    assert_eq!(tensor::ERR_PONDE_INVALID_INDEX, "tensor ponde invalid index");
    assert_eq!(sparsa::ERR_NEGATIVE_DIM, "sparsa shape dimension must be non-negative");
    assert_eq!(sparsa::ERR_NONNIHIL_COUNT_OVERFLOW, "sparsa nonnihil count overflow");
    assert_eq!(sparsa::ERR_ELEMENT_COUNT_OVERFLOW, "sparsa element count overflow");
    assert_eq!(
        sparsa::ERR_CONVERSIO_RANK_MISMATCH,
        "sparsa conversio index rank does not match shape rank"
    );
}

#[test]
fn tensor_shape_math_matches_runtime_semantics() {
    assert!(tensor::tensor_dim_non_negative(0));
    assert!(!tensor::tensor_dim_non_negative(-1));
    assert_eq!(tensor::tensor_shape_element_count(&[2, 3, 4]), Some(24));
    assert_eq!(tensor::tensor_shape_element_count(&[2, -1]), None);
    assert_eq!(tensor::tensor_flat_offset(&[2, 3], &[1, 2]), Some(5));
    assert_eq!(tensor::tensor_flat_offset(&[2, 3], &[1, 3]), None);
    assert_eq!(tensor::tensor_flat_offset(&[2, 3], &[1]), None);
}

#[test]
fn display_helpers_match_runtime_text() {
    assert_eq!(display_fractus(5.0_f64), "5.0");
    assert_eq!(display_fractus(5.5_f64), "5.5");
    assert_eq!(display_bivalens(true, &DisplayTokens::latin()), "verum");
    assert_eq!(display_bivalens(false, &DisplayTokens::latin()), "falsum");
}

#[test]
fn instans_rfc3339_round_trips_at_each_precision() {
    let wire = "1979-05-27T07:32:00Z";
    for (praecisio, expected) in [
        (InstansPraecisio::Secunda, "1979-05-27T07:32:00Z"),
        (InstansPraecisio::Millisecunda, "1979-05-27T07:32:00.000Z"),
        (InstansPraecisio::Microsecunda, "1979-05-27T07:32:00.000000Z"),
        (InstansPraecisio::Nanosecunda, "1979-05-27T07:32:00.000000000Z"),
    ] {
        let value = Instans::from_rfc3339(wire, praecisio).expect("wire parses");
        assert_eq!(value.to_rfc3339(), expected, "round-trip at {praecisio:?}");
    }
}

#[test]
fn instans_from_rfc3339_rejects_malformed() {
    assert!(Instans::from_rfc3339("not a date", InstansPraecisio::Secunda).is_none());
    assert!(Instans::from_rfc3339("1979-05-27", InstansPraecisio::Secunda).is_none());
}

#[test]
fn instans_epoch_integer_interprets_precision_units() {
    // One second at second-precision.
    let s = Instans::from_epoch_integer(1, InstansPraecisio::Secunda);
    assert_eq!(s.to_rfc3339(), "1970-01-01T00:00:01Z");
    // One millisecond at milli-precision.
    let ms = Instans::from_epoch_integer(1, InstansPraecisio::Millisecunda);
    assert_eq!(ms.to_rfc3339(), "1970-01-01T00:00:00.001Z");
}

#[test]
fn instans_ad_praecisionem_truncates_when_narrowing() {
    let fine =
        Instans::from_rfc3339("1979-05-27T07:32:00.987654321Z", InstansPraecisio::Nanosecunda).expect("nanos parse");
    let coarse = fine.ad_praecisionem(InstansPraecisio::Secunda);
    assert_eq!(coarse.to_rfc3339(), "1979-05-27T07:32:00Z");
}

#[test]
fn frame_status_terminal_and_content_classification_matches_runtime() {
    // Mirrors faber_runtime::frame::FrameStatus — the seven canonical variants
    // and their terminal/content classification.
    let all = [
        (FrameStatus::Request, false, false),
        (FrameStatus::Item, false, true),
        (FrameStatus::Byte, false, true),
        (FrameStatus::Bulk, false, true),
        (FrameStatus::Done, true, false),
        (FrameStatus::Error, true, false),
        (FrameStatus::Cancel, true, false),
    ];
    for (status, is_terminal, is_content) in all {
        assert_eq!(status.is_terminal(), is_terminal, "{status:?}: terminal mismatch");
        assert_eq!(status.is_content(), is_content, "{status:?}: content mismatch");
    }
    assert_eq!(all.len(), 7, "exactly seven frame statuses");
}

#[test]
fn instans_compares_at_coarser_precision() {
    let a = Instans::from_rfc3339("1979-05-27T07:32:00.5Z", InstansPraecisio::Millisecunda).unwrap();
    let b = Instans::from_rfc3339("1979-05-27T07:32:00Z", InstansPraecisio::Secunda).unwrap();
    // At second precision the half-second vanishes: equal.
    assert_eq!(a.partial_cmp_at_coarser(b), Some(std::cmp::Ordering::Equal));
}

#[test]
fn tensor_shape_element_count_empty_is_one() {
    // An empty shape [] describes a scalar (0-rank tensor with 1 element).
    assert_eq!(tensor::tensor_shape_element_count(&[]), Some(1));
}

#[test]
fn tensor_flat_offset_negative_index_returns_none() {
    assert_eq!(tensor::tensor_flat_offset(&[3], &[-1]), None);
}

#[test]
fn tensor_dim_non_negative_rejects_large_negative() {
    assert!(!tensor::tensor_dim_non_negative(-1000));
}

#[test]
fn display_fractus_whole_number_shows_decimal() {
    assert_eq!(display_fractus(0.0_f64), "0.0");
    assert_eq!(display_fractus(-3.0_f64), "-3.0");
}
