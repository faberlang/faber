//! GEA3-A1 wire sub-window projection guard tests (Amendment 3).
//!
//! The schema authority (`WireSubWindowProjection`,
//! `validate_carried_sub_window_projection`, `GEA3_WIRE_BUFFER_ABI_V2`)
//! lives in `wire.rs`; these rows prove the fail-closed laws.

use super::{WireSubWindowProjection, validate_carried_sub_window_projection};

fn window(
    element_offset: u64,
    row_width: u64,
    row_count: u64,
    row_stride: u64,
    derived_element_count: u64,
) -> WireSubWindowProjection {
    WireSubWindowProjection {
        buffer: 13,
        element_offset,
        row_width,
        row_count,
        row_stride,
        derived_element_count,
    }
}

fn admit(
    projection: &WireSubWindowProjection,
    element_count: u64,
    producer_element_count: u64,
    write_access: bool,
) -> Result<(), String> {
    validate_carried_sub_window_projection(
        "buffer 'test'",
        element_count,
        producer_element_count,
        write_access,
        projection,
    )
}

#[test]
fn contiguous_read_window_admits() {
    // The pinned decode query slice: head h of rope_q's [1,960] output.
    let projection = window(960 - 64, 64, 1, 64, 64);
    assert!(admit(&projection, 64, 960, false).is_ok());
    assert!(projection.is_contiguous());
    assert_eq!(projection.covering_span(), Some(64));
}

#[test]
fn strided_read_window_admits_and_nests_exactly() {
    // The pinned decode key window: kv head 4 of the [76,320] K cache.
    let projection = window(4 * 64, 64, 76, 320, 76 * 64);
    assert_eq!(projection.derived_element_count, 4_864);
    assert_eq!(projection.covering_span(), Some(75 * 320 + 64));
    assert!(admit(&projection, 4_864, 76 * 320, false).is_ok());
    // Off the end by one element: rejected.
    assert!(admit(&projection, 4_864, 76 * 320 - 1, false).is_err());
}

#[test]
fn write_window_must_be_contiguous() {
    // The attention-output hybrid: head h's [1,64] context result
    // written at offset h·64 of the [·,960] concat buffer.
    let contiguous = window(14 * 64, 64, 1, 64, 64);
    assert!(admit(&contiguous, 64, 960, true).is_ok());
    let scattered = window(0, 64, 15, 960, 15 * 64);
    let error = admit(&scattered, 15 * 64, 960, true).unwrap_err();
    assert!(error.contains("scattered write"), "{error}");
}

#[test]
fn derived_count_disagreements_fail_closed() {
    // Carried derived count disagrees with the geometry.
    let wrong_derived = window(0, 64, 76, 320, 76 * 64 - 1);
    let error = admit(&wrong_derived, 4_864, 76 * 320, false).unwrap_err();
    assert!(error.contains("derived element count"), "{error}");
    // The slot's carried count disagrees with the derived count.
    let wrong_slot = window(0, 64, 76, 320, 4_864);
    let error = admit(&wrong_slot, 76, 76 * 320, false).unwrap_err();
    assert!(error.contains("carried slot count"), "{error}");
}

#[test]
fn degenerate_geometry_fails_closed() {
    assert!(admit(&window(0, 0, 76, 320, 0), 0, 320, false).is_err());
    assert!(admit(&window(0, 64, 0, 64, 0), 0, 320, false).is_err());
    // Row stride below row width would overlap rows.
    let overlap = window(0, 64, 76, 32, 76 * 64);
    let error = admit(&overlap, 76 * 64, 76 * 32, false).unwrap_err();
    assert!(error.contains("pitch"), "{error}");
    // Saturating geometry.
    let saturated = window(0, u64::MAX, 2, u64::MAX, u64::MAX);
    assert!(admit(&saturated, u64::MAX, u64::MAX, false).is_err());
}

#[test]
fn zero_buffer_id_fails_closed() {
    let mut projection = window(0, 64, 1, 64, 64);
    projection.buffer = 0;
    let error = admit(&projection, 64, 960, false).unwrap_err();
    assert!(error.contains("buffer id 0"), "{error}");
}

#[test]
fn abi_successor_is_named() {
    assert_eq!(super::GEA3_WIRE_BUFFER_ABI_V2, "gea3-wire-buffer-v2");
}
