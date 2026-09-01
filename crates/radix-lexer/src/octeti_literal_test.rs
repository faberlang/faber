//! Tests for octeti hex literal decoding and validation.
//! Proves: compact and spaced hex forms decode correctly; invalid
//! characters and odd digit counts are rejected.

use crate::octeti_literal::{OctetiLiteralError, decode_octeti_hex};

#[test]
fn decode_compact_hex_multiple_bytes() {
    assert_eq!(
        decode_octeti_hex("deadbeef").unwrap(),
        vec![0xde, 0xad, 0xbe, 0xef]
    );
}

#[test]
fn decode_spaced_hex_multiple_bytes() {
    assert_eq!(
        decode_octeti_hex("DE AD BE EF").unwrap(),
        vec![0xde, 0xad, 0xbe, 0xef]
    );
}

#[test]
fn decode_spaced_hex_ascii_hello() {
    assert_eq!(
        decode_octeti_hex("48 65 6c 6c 6f").unwrap(),
        b"Hello".to_vec()
    );
}

#[test]
fn decode_single_byte_compact() {
    assert_eq!(decode_octeti_hex("00").unwrap(), vec![0x00]);
}

#[test]
fn decode_empty_hex() {
    assert_eq!(decode_octeti_hex("").unwrap(), Vec::<u8>::new());
}

#[test]
fn reject_odd_digit_count() {
    assert_eq!(
        decode_octeti_hex("abc"),
        Err(OctetiLiteralError::OddDigitCount)
    );
}

#[test]
fn reject_invalid_hex_char() {
    assert_eq!(
        decode_octeti_hex("gg"),
        Err(OctetiLiteralError::InvalidCharacter('g'))
    );
}

#[test]
fn reject_invalid_char_in_spaced_form() {
    assert_eq!(
        decode_octeti_hex("48 65 gg"),
        Err(OctetiLiteralError::InvalidCharacter('g'))
    );
}

#[test]
fn reject_0x_prefix() {
    assert_eq!(
        decode_octeti_hex("0xdead"),
        Err(OctetiLiteralError::InvalidCharacter('x'))
    );
}
