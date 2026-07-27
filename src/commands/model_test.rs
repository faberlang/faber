//! Tests for `faber model` subcommands.
//!
//! All tests use hand-crafted safetensors byte buffers — no external fixtures.

use super::{parse_safetensors_metadata, TensorInfo};

// ---------------------------------------------------------------------------
// Safetensors byte-buffer helpers
// ---------------------------------------------------------------------------

/// Build a valid minimal safetensors byte buffer.
///
/// `json_str` is the JSON header content (without padding). The function
/// computes the 8-byte little-endian length prefix, appends the JSON bytes,
/// and pads with 0x20 to an 8-byte boundary.
fn build_safetensors(json_str: &str) -> Vec<u8> {
    let json_bytes = json_str.as_bytes();
    let padded_len = (json_bytes.len() + 7) & !7;
    let mut padded = json_bytes.to_vec();
    padded.resize(padded_len, 0x20);

    let mut buf = Vec::with_capacity(8 + padded.len());
    buf.extend_from_slice(&(padded.len() as u64).to_le_bytes());
    buf.extend_from_slice(&padded);
    buf
}

/// Build a safetensors buffer with a single F32 tensor.
fn single_tensor_f32() -> Vec<u8> {
    build_safetensors(r#"{"test.tensor":{"dtype":"F32","shape":[2,3],"data_offsets":[0,24]}}"#)
}

/// Build a safetensors buffer with __metadata__ and multiple tensors.
fn multi_tensor_with_metadata() -> Vec<u8> {
    build_safetensors(
        r#"{"__metadata__":{"format":"pt","model":"bert-tiny"},"w":{"dtype":"F32","shape":[128,128],"data_offsets":[0,65536]},"b":{"dtype":"F32","shape":[128],"data_offsets":[65536,65664]}}"#,
    )
}

// ---------------------------------------------------------------------------
// parse_safetensors_metadata tests
// ---------------------------------------------------------------------------

#[test]
fn parse_valid_single_tensor() {
    let data = single_tensor_f32();
    let header = parse_safetensors_metadata(&data).expect("parse valid safetensors");
    assert_eq!(header.tensors.len(), 1);
    assert!(header.metadata.is_empty());

    let t = &header.tensors[0];
    assert_eq!(t.name, "test.tensor");
    assert_eq!(t.dtype, "F32");
    assert_eq!(t.shape, vec![2, 3]);
    assert_eq!(t.data_offsets, vec![0, 24]);
    assert_eq!(t.element_count, 6);
}

#[test]
fn parse_valid_metadata_and_multiple_tensors() {
    let data = multi_tensor_with_metadata();
    let header = parse_safetensors_metadata(&data).expect("parse safetensors with metadata");

    assert_eq!(header.metadata.len(), 2);
    let meta_map: std::collections::HashMap<&str, &str> = header
        .metadata
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(meta_map.get("format"), Some(&"pt"));
    assert_eq!(meta_map.get("model"), Some(&"bert-tiny"));

    assert_eq!(header.tensors.len(), 2);

    // serde_json::Map iterates alphabetically without the `preserve_order`
    // feature, so tensor order is not guaranteed to match file/insertion order.
    // Look up by name instead of assuming position.
    let w = header
        .tensors
        .iter()
        .find(|t| t.name == "w")
        .expect("tensor `w` present");
    assert_eq!(w.dtype, "F32");
    assert_eq!(w.shape, vec![128, 128]);
    assert_eq!(w.data_offsets, vec![0, 65536]);
    assert_eq!(w.element_count, 16384);

    let b = header
        .tensors
        .iter()
        .find(|t| t.name == "b")
        .expect("tensor `b` present");
    assert_eq!(b.dtype, "F32");
    assert_eq!(b.shape, vec![128]);
    assert_eq!(b.data_offsets, vec![65536, 65664]);
    assert_eq!(b.element_count, 128);
}

#[test]
fn parse_empty_buffer_returns_error() {
    let err = parse_safetensors_metadata(b"").expect_err("empty buffer");
    assert!(err.contains("too short"));
}

#[test]
fn parse_truncated_header_returns_error() {
    let err =
        parse_safetensors_metadata(b"\x08\x00\x00\x00\x00\x00\x00\x00").expect_err("truncated");
    assert!(err.contains("too short"));
}

#[test]
fn parse_zero_header_length_returns_error() {
    let err = parse_safetensors_metadata(&[0u8; 8]).expect_err("zero header length");
    assert!(err.contains("zero"));
}

#[test]
fn parse_non_utf8_header_returns_error() {
    let mut buf = vec![4u8, 0, 0, 0, 0, 0, 0, 0];
    buf.extend_from_slice(&[0xff, 0xfe, 0x80, 0x00]);
    let err = parse_safetensors_metadata(&buf).expect_err("invalid UTF-8");
    assert!(err.contains("UTF-8"));
}

#[test]
fn parse_bad_json_header_returns_error() {
    let data = build_safetensors("not json at all");
    let err = parse_safetensors_metadata(&data).expect_err("bad JSON");
    assert!(err.contains("JSON"));
}

#[test]
fn parse_json_root_not_object_returns_error() {
    let data = build_safetensors(r#""just a string""#);
    let err = parse_safetensors_metadata(&data).expect_err("root is string");
    assert!(err.contains("not an object"));
}

#[test]
fn parse_tensor_missing_dtype_returns_error() {
    let data = build_safetensors(r#"{"t":{"shape":[1],"data_offsets":[0,4]}}"#);
    let err = parse_safetensors_metadata(&data).expect_err("missing dtype");
    assert!(err.contains("dtype"));
}

#[test]
fn parse_tensor_missing_shape_returns_error() {
    let data = build_safetensors(r#"{"t":{"dtype":"F32","data_offsets":[0,4]}}"#);
    let err = parse_safetensors_metadata(&data).expect_err("missing shape");
    assert!(err.contains("shape"));
}

#[test]
fn parse_tensor_missing_data_offsets_returns_error() {
    let data = build_safetensors(r#"{"t":{"dtype":"F32","shape":[1]}}"#);
    let err = parse_safetensors_metadata(&data).expect_err("missing data_offsets");
    assert!(err.contains("data_offsets"));
}

#[test]
fn parse_tensor_with_non_integer_shape_value_returns_error() {
    let data =
        build_safetensors(r#"{"t":{"dtype":"F32","shape":["dynamic"],"data_offsets":[0,4]}}"#);
    let err = parse_safetensors_metadata(&data).expect_err("non-integer shape");
    assert!(err.contains("shape"));
}

#[test]
fn parse_tensor_entry_is_not_object_returns_error() {
    let data = build_safetensors(r#"{"t":123}"#);
    let err = parse_safetensors_metadata(&data).expect_err("tensor not object");
    assert!(err.contains("not a JSON object"));
}

#[test]
fn parse_scalar_tensor_zero_dims() {
    let data = build_safetensors(r#"{"scalar":{"dtype":"F32","shape":[],"data_offsets":[0,4]}}"#);
    let header = parse_safetensors_metadata(&data).expect("parse scalar");
    assert_eq!(header.tensors[0].shape.len(), 0);
    assert_eq!(header.tensors[0].element_count, 1);
}

#[test]
fn parse_int8_dtype() {
    let data = build_safetensors(r#"{"t":{"dtype":"I8","shape":[4],"data_offsets":[0,4]}}"#);
    let header = parse_safetensors_metadata(&data).expect("parse I8");
    assert_eq!(header.tensors[0].dtype, "I8");
}

#[test]
fn parse_buffer_with_padding() {
    let json = r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#;
    let data = build_safetensors(json);
    let header = parse_safetensors_metadata(&data).expect("parse padded");
    assert_eq!(header.tensors.len(), 1);
    assert_eq!(header.tensors[0].name, "a");
}

#[test]
fn parse_empty_header_no_tensors() {
    let data = build_safetensors("{}");
    let header = parse_safetensors_metadata(&data).expect("parse empty header");
    assert_eq!(header.tensors.len(), 0);
    assert!(header.metadata.is_empty());
}

#[test]
fn parse_tensor_with_zero_dimension_shape() {
    let data =
        build_safetensors(r#"{"t":{"dtype":"F32","shape":[0],"data_offsets":[0,0]}}"#);
    let header = parse_safetensors_metadata(&data).expect("parse zero-dim tensor");
    assert_eq!(header.tensors[0].shape, vec![0]);
    assert_eq!(header.tensors[0].element_count, 0);
}

#[test]
fn parse_f64_dtype() {
    let data = build_safetensors(r#"{"t":{"dtype":"F64","shape":[2],"data_offsets":[0,16]}}"#);
    let header = parse_safetensors_metadata(&data).expect("parse F64");
    assert_eq!(header.tensors[0].dtype, "F64");
}

// ---------------------------------------------------------------------------
// Display format tests
// ---------------------------------------------------------------------------

#[test]
fn tensor_display_format() {
    let t = TensorInfo {
        name: "query.weight".into(),
        dtype: "F32".into(),
        shape: vec![128, 128],
        data_offsets: vec![0, 65536],
        element_count: 16384,
    };
    let s = t.to_string();
    assert!(s.contains("query.weight"));
    assert!(s.contains("F32"));
    assert!(s.contains("[128, 128]"));
    assert!(s.contains("[0, 65536]"));
    assert!(s.contains("16384"));
}
