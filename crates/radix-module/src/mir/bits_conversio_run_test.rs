//! Need, unit 1.4a: same-width `bN ↔ uN` conversions (scalars and
//! lists, no `via` clause) run on the MIR runner and keep the bit pattern.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn ok(body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
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

fn refused(body: &str) -> String {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "bits.fab", &source, &mut host) {
        Err(RunSourceError::Stepper(errors)) => panic!("runtime error: {errors:?}"),
        Err(other) => format!("{other:?}"),
        Ok(()) => panic!("{body}: expected a refusal"),
    }
}

#[test]
fn width_changing_and_unsettled_conversions_are_refused_by_name() {
    let cases = [
        ("fixum b8 a ← 1\nnota a ↦ u16", "bits_conversion_width_mismatch"),
        ("fixum b16 a ← 1\nnota a ↦ u8", "bits_conversion_width_mismatch"),
        ("fixum b8 a ← 1\nnota a ↦ numerus", "bits_conversion_width_mismatch"),
        ("fixum lista<b8> a ← [1]\nnota a ↦ lista<b16>", "bits_conversion_width_mismatch"),
        ("fixum b8 a ← 1\nnota a ↦ i8", "bits_signed_conversion_unsettled"),
        ("fixum b32 a ← 1\nnota a ↦ f32", "bits_float_conversion_unsettled"),
        ("fixum b8 a ← 1\nnota a ↦ u8 via Le", "bits_via_clause_not_allowed"),
        ("fixum u8 a ← 1\nnota a ↦ b8 via Be", "bits_via_clause_not_allowed"),
    ];
    for (body, issue) in cases {
        let message = refused(body);
        assert!(message.contains(issue), "{body}: {message}");
    }
}
