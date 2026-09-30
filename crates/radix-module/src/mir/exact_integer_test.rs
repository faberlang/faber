//! d11-6 U5: exact integers, floor division, shifts, `¬`, store policies and
//! exact cross-family comparison, end to end through the MIR runner. Each test
//! names the ruling it pins; the values are the delivery spec's §4.4 probes.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "exact.fab", &source, &mut host) {
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

fn trap(body: &str) -> (Vec<String>, String, String) {
    let (out, errors) = run(body);
    let (issue, message) = errors
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no trap: {body}"));
    (out, issue, message)
}

#[test]
fn probes_store_or_widen_by_value() {
    // p01, p03, p12: arithmetic is wide; only the final store is checked.
    assert_eq!(ok("varia numerus<u8> x ← 250\nfixum numerus<u8> y ← x + 2\nnota y"), ["252"]);
    assert_eq!(ok("varia numerus<u8> x ← 250\nfixum numerus w ← x + 10\nnota w"), ["260"]);
    assert_eq!(
        ok("varia numerus<u8> x ← 200\nfixum numerus<u8> y ← x * 2 / 4\nnota y"),
        ["100"]
    );
    // p18
    assert_eq!(ok("varia numerus<i8> x ← -128\nfixum numerus y ← -x\nnota y"), ["128"]);
}

#[test]
fn probes_trap_at_the_store_with_the_store_identity() {
    // p02, p04, p08b (ruling 5): identity is distinct from arithmetic overflow.
    for body in [
        "varia numerus<u8> x ← 250\nnota \"a\"\nfixum numerus<u8> y ← x + 10\nnota y",
        "varia numerus<u8> x ← 0\nnota \"a\"\nfixum numerus<u8> y ← x + 300\nnota y",
        "varia numerus<u8> x ← 250\nnota \"a\"\nfixum numerus<u8> y ← x ⇐ 2\nnota y",
    ] {
        let (out, issue, message) = trap(body);
        assert_eq!(out, ["a"]);
        assert_eq!(issue, "implicit_store_out_of_range", "{body}");
        assert!(message.contains("does not fit in `u8`") && message.contains("`y`"), "{message}");
    }
}

#[test]
fn increment_of_a_narrow_slot_is_a_store() {
    // p10 (row 4).
    let (out, issue, message) = trap("varia numerus<u8> x ← 255\nnota \"a\"\nx ↑\nnota x");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("256") && message.contains("`x`"), "{message}");
    assert_eq!(ok("varia numerus<u8> x ← 254\nx ↑\nnota x"), ["255"]);
}

#[test]
fn inferred_slot_trap_names_the_source_expression() {
    // ruling 19(i): the message points at the inferring expression.
    let (_, _, message) = trap("varia numerus<u8> a ← 200\nvaria _ total ← a + 1\ntotal ← total + 100000");
    assert!(message.contains("inferred"), "{message}");
}

#[test]
fn floor_division_and_remainder() {
    // ruling 14.
    assert_eq!(
        ok("varia numerus a ← -7\nvaria numerus b ← 2\nnota a / b\nnota a % b\nnota 7 / -2\nnota 7 % -2"),
        ["-4", "1", "-4", "-1"]
    );
    let (out, _, _) = trap("varia numerus z ← 0\nnota \"a\"\nnota 1 / z");
    assert_eq!(out, ["a"]);
}

#[test]
fn min_over_minus_one_is_exact_and_only_the_i64_store_traps() {
    // ruling 14 / 17: 2^63 is a valid intermediate and a valid u64.
    let out =
        ok("varia numerus m ← -9223372036854775807 - 1\nvaria numerus d ← -1\nfixum numerus<u64> q ← m / d\nnota q");
    assert_eq!(out, ["9223372036854775808"]);
    let (_, issue, _) =
        trap("varia numerus m ← -9223372036854775807 - 1\nvaria numerus d ← -1\nfixum numerus r ← m / d\nnota r");
    assert_eq!(issue, "implicit_store_out_of_range");
}

#[test]
fn bitnot_is_minus_x_minus_one() {
    // rulings 9, 19(b).
    assert_eq!(
        ok("varia numerus<u8> x ← 250\nfixum numerus<i16> y ← ¬x\nnota y\nnota ¬0"),
        ["-251", "-1"]
    );
    assert_eq!(
        ok("varia numerus<u64> a ← 9223372036854775807\nnota ¬a"),
        ["-9223372036854775808"]
    );
    let (_, issue, _) = trap("varia numerus<u64> a ← 9223372036854775808\nnota ¬a");
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn bitnot_of_u64_max_is_past_the_range() {
    let (out, issue, _) = trap("varia numerus<u64> a ← 18446744073709551615\nnota \"a\"\nnota ¬a");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn shifts_are_unmasked() {
    // ruling 10 and the audit's reproductions.
    assert_eq!(
        ok("varia numerus<u8> a ← 1\nvaria numerus<u8> n ← 63\nfixum numerus<u64> y ← a ⇐ n\nnota y"),
        ["9223372036854775808"]
    );
    for n in ["64"] {
        let body = format!("varia numerus<u8> a ← 1\nvaria numerus<u8> n ← {n}\nnota \"a\"\nnota a ⇐ n");
        assert_eq!(trap(&body).0, ["a"]);
    }
    let (_, _, _) = trap("varia numerus<u8> a ← 255\nvaria numerus<u8> n ← 60\nnota \"a\"\nnota a ⇐ n");
    assert_eq!(
        ok("varia numerus a ← 5\nvaria numerus n ← 200\nnota a ⇒ n\nvaria numerus m ← -5\nnota m ⇒ n\nnota m ⇒ 1"),
        ["0", "-1", "-3"]
    );
    let (out, _, message) = trap("varia numerus a ← 5\nvaria numerus n ← -1\nnota \"a\"\nnota a ⇒ n");
    assert_eq!(out, ["a"]);
    assert!(message.contains("negative shift count"), "{message}");
}

#[test]
fn wrapping_and_saturating_stores_apply_the_policy_at_runtime() {
    // ruling 3 / 17b, non-constant values.
    assert_eq!(
        ok(
            "varia modulus<u32> n ← 300\nfixum modulus<u8> w ← n\nnota w\nvaria modulus<u32> m ← 4294967295\nfixum modulus<u8> v ← m\nnota v"
        ),
        ["44", "255"]
    );
    assert_eq!(
        ok(
            "varia saturatus<u16> n ← 300\nfixum saturatus<u8> w ← n\nnota w\nvaria saturatus<i16> k ← -500\nfixum saturatus<i8> u ← k\nnota u"
        ),
        ["255", "-128"]
    );
}

#[test]
fn wrapping_shift_count_is_not_masked() {
    // ruling 10: `1 ⇐ 33` on `wrapping<u32>` is 0.
    assert_eq!(
        ok("varia modulus<u32> a ← 1\nvaria modulus<u32> n ← 33\nnota a ⇐ n\nnota a ⇒ n"),
        ["0", "0"]
    );
}

#[test]
fn true_division_of_words_converts_and_traps_on_zero() {
    // audit item: `÷` on wrapping / saturating operands is real division.
    assert_eq!(
        ok("varia modulus<u8> a ← 7\nvaria modulus<u8> b ← 2\nfixum fractus q ← a ÷ b\nnota q"),
        ["3.5"]
    );
    let (out, _, _) = trap("varia modulus<u8> a ← 7\nvaria modulus<u8> b ← 0\nnota \"a\"\nnota a ÷ b");
    assert_eq!(out, ["a"]);
}

#[test]
fn integer_float_comparison_is_exact() {
    // ruling 22.
    assert_eq!(
        ok(
            "varia numerus a ← 9007199254740993\nvaria fractus f ← 9007199254740992.0\nnota a > f\nnota a ≅ f\nnota a ≇ f"
        ),
        ["verum", "falsum", "verum"]
    );
}

#[test]
fn decimal_float_comparison_is_exact() {
    // ruling 22: 0.1d64 is below the float nearest 0.1.
    assert_eq!(
        ok(
            "varia d64 d ← 0.1\nvaria fractus f ← 0.1\nvaria fractus g ← 0.5\nnota d < f\nnota d > f\nnota d ≅ f\nnota d ≺ g"
        ),
        ["verum", "falsum", "falsum", "verum"]
    );
}

#[test]
fn u64_arithmetic_past_the_range_traps() {
    let (out, issue, _) =
        trap("varia numerus<u64> a ← 18446744073709551615\nvaria numerus<u64> b ← 1\nnota \"a\"\nnota a + b");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn explicit_narrowing_stays_recoverable() {
    assert_eq!(
        ok("varia numerus n ← 300\nfixum numerus<u8> y ← n ↦ numerus<u8> ⊥ 7\nnota y"),
        ["7"]
    );
}

#[test]
fn true_division_width_follows_the_operands() {
    // ruling 23: 8- and 16-bit operands divide in f32; 32/64-bit in f64.
    assert_eq!(
        ok("varia numerus<u8> a ← 1\nvaria numerus<u8> b ← 3\nnota a ÷ b"),
        ["0.33333334"]
    );
    assert_eq!(
        ok("varia numerus<i16> a ← 1\nvaria numerus<i32> b ← 3\nnota a ÷ b"),
        ["0.3333333333333333"]
    );
    assert_eq!(ok("varia numerus<u8> a ← 1\nnota a ÷ 3"), ["0.33333334"]);
    assert_eq!(ok("nota 7 ÷ 2\nnota 1 ÷ 3"), ["3.5", "0.3333333333333333"]);
    // A sub-expression keys on its range type: (u8 + u8) is u16, still f32.
    assert_eq!(
        ok(
            "varia numerus<u8> a ← 200\nvaria numerus<u8> b ← 100\nvaria numerus<u8> c ← 7\nfixum _ q ← (a + b) ÷ c\nnota q"
        ),
        ["42.857143"]
    );
    let (out, _, _) = trap("varia numerus<u8> a ← 1\nvaria numerus<u8> b ← 0\nnota \"a\"\nnota a ÷ b");
    assert_eq!(out, ["a"]);
}
