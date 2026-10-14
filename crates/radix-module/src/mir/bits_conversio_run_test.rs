//! Need, unit 1.4a: same-width `bN ↔ uN | iN | fN` conversions (scalars and
//! lists, no `via` clause) run on the MIR runner and keep the bit pattern.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn ok(body: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "bits.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

#[test]
fn bits_to_unsigned_keeps_the_pattern_at_every_width() {
    assert_eq!(ok("fixum b8 a ← 0xff\nfixum u8 n ← a ↦ u8\nnota a\nnota n"), ["0xff", "255"]);
    assert_eq!(ok("fixum b16 a ← 0xabcd\nnota a ↦ u16"), ["43981"]);
    assert_eq!(ok("fixum b32 a ← 0xffffffff\nnota a ↦ u32"), ["4294967295"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota a ↦ u64"), ["18446744073709551615"]);
}

#[test]
fn unsigned_to_bits_keeps_the_pattern_and_prints_hex() {
    assert_eq!(ok("fixum u8 n ← 255\nnota n ↦ b8"), ["0xff"]);
    assert_eq!(ok("fixum u16 n ← 127\nnota n ↦ b16"), ["0x007f"]);
    assert_eq!(ok("fixum u32 n ← 1\nnota n ↦ b32"), ["0x00000001"]);
    assert_eq!(ok("fixum u64 n ← 18446744073709551615\nnota n ↦ b64"), ["0xffffffffffffffff"]);
}

#[test]
fn lists_convert_element_wise_at_the_same_width_in_both_directions() {
    assert_eq!(ok("fixum lista<b16> l ← [0x0001, 0xffff]\nnota l ↦ lista<u16>"), ["[1, 65535]"]);
    assert_eq!(ok("fixum lista<u8> l ← [1, 255]\nnota l ↦ lista<b8>"), ["[0x01, 0xff]"]);
}

#[test]
fn signed_conversions_read_two_complement_in_both_directions() {
    assert_eq!(ok("fixum b8 a ← 0xff\nnota a ↦ i8"), ["-1"]);
    assert_eq!(ok("fixum i8 n ← -1\nnota n ↦ b8"), ["0xff"]);
    assert_eq!(ok("fixum b16 a ← 0x8000\nnota a ↦ i16"), ["-32768"]);
    assert_eq!(ok("fixum i32 n ← -2\nnota n ↦ b32"), ["0xfffffffe"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota a ↦ i64"), ["-1"]);
    assert_eq!(ok("fixum lista<b8> l ← [0x01, 0xff]\nnota l ↦ lista<i8>"), ["[1, -1]"]);
}

#[test]
fn float_conversions_read_and_write_the_pattern() {
    assert_eq!(ok("fixum b16 a ← 0xc000\nnota a ↦ f16"), ["-2.0"]);
    assert_eq!(ok("fixum b32 a ← 0x3f800000\nnota a ↦ f32"), ["1.0"]);
    assert_eq!(ok("fixum b64 a ← 0x3ff8000000000000\nnota a ↦ f64"), ["1.5"]);
    assert_eq!(ok("fixum b16 a ← 0x3fc0\nnota a ↦ bf16"), ["1.5"]);
    assert_eq!(ok("fixum f32 x ← 1.5\nnota x ↦ b32"), ["0x3fc00000"]);
    assert_eq!(ok("fixum f64 x ← 1.5\nnota x ↦ b64"), ["0x3ff8000000000000"]);
    assert_eq!(ok("fixum f16 x ← 1.5\nnota x ↦ b16"), ["0x3e00"]);
    assert_eq!(ok("fixum bf16 x ← 1.5\nnota x ↦ b16"), ["0x3fc0"]);
}

#[test]
fn number_to_float_stays_a_value_conversion() {
    assert_eq!(ok("fixum u16 n ← 15872\nnota n ↦ f32"), ["15872.0"]);
}

fn refused(body: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "bits.fab", &source, &mut host) {
        Err(RunSourceError::Stepper(errors)) => panic!("runtime error: {errors:?}"),
        Err(other) => format!("{other:?}"),
        Ok(()) => panic!("{body}: expected a refusal"),
    }
}

#[test]
fn width_changing_conversions_are_refused_by_name() {
    let cases = [
        ("fixum b8 a ← 1\nnota a ↦ u16", "bits_conversion_width_mismatch"),
        ("fixum b16 a ← 1\nnota a ↦ u8", "bits_conversion_width_mismatch"),
        ("fixum b8 a ← 1\nnota a ↦ numerus", "bits_conversion_width_mismatch"),
        ("fixum lista<b8> a ← [1]\nnota a ↦ lista<b16>", "bits_via_clause_required"),
        ("fixum b8 a ← 1\nnota a ↦ i16", "bits_conversion_width_mismatch"),
        ("fixum b32 a ← 1\nnota a ↦ f64", "bits_conversion_width_mismatch"),
        ("fixum b8 a ← 1\nnota a ↦ u8 via Le", "bits_via_clause_not_allowed"),
        ("fixum u8 a ← 1\nnota a ↦ b8 via Be", "bits_via_clause_not_allowed"),
    ];
    for (body, issue) in cases {
        let message = refused(body);
        assert!(message.contains(issue), "{body}: {message}");
    }
}

#[test]
fn bits_list_width_change_decodes_and_encodes_through_the_byte_order() {
    let le = "fixum lista<b8> a ← [0x01, 0x02, 0x03, 0x04]\nnota a ↦ lista<u16> via Le";
    assert_eq!(ok(le), ["[513, 1027]"]);
    let be = "fixum lista<b8> a ← [0x01, 0x02, 0x03, 0x04]\nnota a ↦ lista<b16> via Be";
    assert_eq!(ok(be), ["[0x0102, 0x0304]"]);
    let encode = "fixum lista<u16> a ← [0x0102, 0x0304]\nnota a ↦ lista<b8> via Le";
    assert_eq!(ok(encode), ["[0x02, 0x01, 0x04, 0x03]"]);
    let same = "fixum lista<b8> a ← [0x01, 0xff]\nnota a ↦ lista<u8>";
    assert_eq!(ok(same), ["[1, 255]"]);
}

#[test]
fn bits_lists_decode_and_encode_text_at_widths_8_16_32() {
    // UTF-8: a multi-byte character, round trip.
    assert_eq!(ok("fixum lista<b8> l ← [0x68, 0xc3, 0xa9]\nnota l ↦ textus ⊥ \"?\""), ["hé"]);
    assert_eq!(ok("fixum textus s ← \"hé\"\nnota s ↦ lista<b8>"), ["[0x68, 0xc3, 0xa9]"]);
    // An invalid UTF-8 byte string is the recoverable failure.
    assert_eq!(ok("fixum lista<b8> l ← [0xff, 0x41]\nnota l ↦ textus ⊥ \"?\""), ["?"]);
    // UTF-16: a character outside the basic plane is a surrogate pair.
    assert_eq!(
        ok("fixum lista<b16> l ← [0xd83d, 0xde00]\nnota l ↦ textus ⊥ \"?\""),
        ["\u{1F600}"]
    );
    assert_eq!(
        ok("fixum textus s ← \"a\u{1F600}\"\nnota s ↦ lista<b16>"),
        ["[0x0061, 0xd83d, 0xde00]"]
    );
    assert_eq!(ok("fixum lista<b16> l ← [0xd800]\nnota l ↦ textus ⊥ \"?\""), ["?"]);
    // UTF-32.
    assert_eq!(
        ok("fixum textus s ← \"a\u{1F600}\"\nnota s ↦ lista<b32>"),
        ["[0x00000061, 0x0001f600]"]
    );
    assert_eq!(ok("fixum lista<b32> l ← [0x110000]\nnota l ↦ textus ⊥ \"?\""), ["?"]);
}
