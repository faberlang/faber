//! Need, unit 1.3b: operations on the raw-bits markers
//! `b8 b16 b32 b64` run on the MIR runner at exactly the type's width
//! (operator rulings A3 / B3 / B12), end to end through source.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "bits.fab", &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{body}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(body: &str) -> Vec<String> {
    let (out, errors) = run(body);
    assert!(errors.is_empty(), "{body}: {errors:?}");
    out
}

#[test]
fn bits_not_flips_exactly_the_width() {
    assert_eq!(ok("fixum b8 a ← 0x0f\nnota ¬ a"), ["0xf0"]);
    assert_eq!(ok("fixum b16 a ← 0x00ff\nnota ¬ a"), ["0xff00"]);
    assert_eq!(ok("fixum b32 a ← 0x0000ffff\nnota ¬ a"), ["0xffff0000"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota ¬ a"), ["0x0000000000000000"]);
}

#[test]
fn bits_left_shift_drops_bits_off_the_top() {
    assert_eq!(ok("fixum b8 a ← 0x81\nnota a ⇐ 1"), ["0x02"]);
    assert_eq!(ok("fixum b16 a ← 0x8001\nnota a ⇐ 4"), ["0x0010"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota a ⇐ 4"), ["0xfffffffffffffff0"]);
}

#[test]
fn bits_right_shift_fills_with_zeros() {
    assert_eq!(ok("fixum b8 a ← 0x80\nnota a ⇒ 1"), ["0x40"]);
    assert_eq!(ok("fixum b8 a ← 0xf0\nnota a ⇒ 4"), ["0x0f"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota a ⇒ 4"), ["0x0fffffffffffffff"]);
}

#[test]
fn bits_shift_by_the_width_or_more_gives_zero_without_wrapping_the_count() {
    assert_eq!(ok("fixum b8 a ← 0x81\nnota a ⇐ 8"), ["0x00"]);
    assert_eq!(ok("fixum b8 a ← 0x81\nnota a ⇒ 8"), ["0x00"]);
    assert_eq!(ok("fixum b8 a ← 0x81\nnota a ⇒ 9"), ["0x00"]);
    assert_eq!(ok("fixum b8 a ← 0x81\nnota a ⇐ 100"), ["0x00"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota a ⇐ 64"), ["0x0000000000000000"]);
    assert_eq!(ok("fixum b64 a ← 0xffffffffffffffff\nnota a ⇒ 63"), ["0x0000000000000001"]);
}

#[test]
fn bits_shift_round_trip_clears_the_high_bits_at_the_width() {
    // A3: `(b ⇐ 4) ⇒ 4` on a b8 clears the high four bits.
    assert_eq!(ok("fixum b8 a ← 0xab\nfixum b8 t ← (a ⇐ 4) ⇒ 4\nnota t"), ["0x0b"]);
}

#[test]
fn bits_negative_run_time_count_is_a_named_run_time_error() {
    let (out, errors) = run("fixum b8 a ← 0x0f\nfixum numerus n ← 0 - 1\nnota \"a\"\nnota a ⇐ n");
    assert_eq!(out, ["a"]);
    let (_, message) = errors.into_iter().next().expect("a run-time error");
    assert!(message.contains("negative shift count"), "{message}");
    let (_, errors) = run("fixum b8 a ← 0x0f\nfixum numerus n ← 0 - 1\nnota a ⇒ n");
    assert!(
        errors
            .first()
            .is_some_and(|(_, message)| message.contains("negative shift count"))
    );
}

#[test]
fn bits_and_or_xor_act_per_bit() {
    assert_eq!(ok("fixum b8 a ← 0xf0\nfixum b8 b ← 0x3c\nnota a ∧ b"), ["0x30"]);
    assert_eq!(ok("fixum b8 a ← 0xf0\nfixum b8 b ← 0x3c\nnota a ∨ b"), ["0xfc"]);
    assert_eq!(ok("fixum b8 a ← 0xf0\nfixum b8 b ← 0x3c\nnota a ⊻ b"), ["0xcc"]);
    assert_eq!(
        ok("fixum b64 a ← 0xff00ff00ff00ff00\nfixum b64 b ← 0x0ff00ff00ff00ff0\nnota a ⊻ b"),
        ["0xf0f0f0f0f0f0f0f0"]
    );
}

#[test]
fn bits_order_by_unsigned_pattern_and_equality_by_value() {
    assert_eq!(ok("fixum b8 a ← 0x80\nfixum b8 b ← 0x7f\nnota a ≻ b"), ["verum"]);
    assert_eq!(ok("fixum b8 a ← 0x80\nfixum b8 b ← 0x7f\nnota a ≺ b"), ["falsum"]);
    assert_eq!(ok("fixum b8 a ← 0x80\nnota a ≥ 0x80"), ["verum"]);
    assert_eq!(ok("fixum b8 a ← 0x80\nnota a ≡ 0x80"), ["verum"]);
    assert_eq!(ok("fixum b8 a ← 0x80\nnota a ≢ 0x80"), ["falsum"]);
    assert_eq!(
        ok("fixum b64 a ← 0x8000000000000000\nfixum b64 b ← 0x7fffffffffffffff\nnota a ≻ b"),
        ["verum"]
    );
}

#[test]
fn bits_literal_in_decimal_and_binary_becomes_the_stored_pattern() {
    assert_eq!(ok("fixum b8 a ← 127\nnota a"), ["0x7f"]);
    assert_eq!(ok("fixum b8 a ← 0b1000_0001\nnota a"), ["0x81"]);
}

#[test]
fn bits_print_as_hex_at_the_full_width() {
    assert_eq!(ok("fixum b8 a ← 0x7f\nnota a"), ["0x7f"]);
    assert_eq!(ok("fixum b16 a ← 0x7f\nnota a"), ["0x007f"]);
    assert_eq!(ok("fixum b32 a ← 0x7f\nnota a"), ["0x0000007f"]);
    assert_eq!(ok("fixum b64 a ← 0x7f\nnota a"), ["0x000000000000007f"]);
}

fn ok_program(source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    run_source(&session, "bits.fab", source, &mut host).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    host.stdout_lines
}

#[test]
fn bits_print_as_hex_inside_lists_tuples_genus_fields_and_templates() {
    assert_eq!(ok("fixum lista<b8> xs ← [0x7f, 0x01]\nnota xs"), ["[0x7f, 0x01]"]);
    assert_eq!(ok("fixum b16 a ← 0x7f\nnota \"v=§\"(a)"), ["v=0x007f"]);
    let genus = "genus Reg { varia b16 value }\nincipit {\nfixum Reg r ← Reg { value = 0x7f }\nnota r\n}\n";
    assert_eq!(ok_program(genus), ["Reg { value = 0x007f }"]);
}
