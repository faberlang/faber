//! d11-6 U5: exact integers, floor division, shifts, `¬`, store policies and
//! exact cross-family comparison, end to end through the MIR runner. Each test
//! names the ruling it pins; the values are the delivery spec's §4.4 probes.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
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
    assert_eq!(ok("varia u8 x ← 250\nsit u8 y ← x + 2\nnota y"), ["252"]);
    assert_eq!(ok("varia u8 x ← 250\nsit numerus w ← x + 10\nnota w"), ["260"]);
    assert_eq!(ok("varia u8 x ← 200\nsit u8 y ← x * 2 / 4\nnota y"), ["100"]);
    // p18
    assert_eq!(ok("varia i8 x ← -128\nsit numerus y ← -x\nnota y"), ["128"]);
}

#[test]
fn probes_trap_at_the_store_with_the_store_identity() {
    // p02, p04, p08b (ruling 5): identity is distinct from arithmetic overflow.
    for body in [
        "varia u8 x ← 250\nnota \"a\"\nsit u8 y ← x + 10\nnota y",
        "varia u8 x ← 0\nnota \"a\"\nsit u8 y ← x + 300\nnota y",
        "varia u8 x ← 250\nnota \"a\"\nsit u8 y ← x ⇐ 2\nnota y",
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
    let (out, issue, message) = trap("varia u8 x ← 255\nnota \"a\"\nx ↑\nnota x");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("256") && message.contains("`x`"), "{message}");
    assert_eq!(ok("varia u8 x ← 254\nx ↑\nnota x"), ["255"]);
}

#[test]
fn inferred_slot_trap_names_the_source_expression() {
    // ruling 19(i): the message points at the inferring expression.
    let (_, _, message) = trap("varia u8 a ← 200\nvaria _ total ← a + 1\ntotal ← total + 100000");
    assert!(message.contains("inferred"), "{message}");
    assert!(message.contains("from `a + 1`"), "{message}");
    assert!(!message.contains("bytes"), "{message}");
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
    let out = ok("varia numerus m ← -9223372036854775807 - 1\nvaria numerus d ← -1\nsit u64 q ← m / d\nnota q");
    assert_eq!(out, ["9223372036854775808"]);
    let (_, issue, _) =
        trap("varia numerus m ← -9223372036854775807 - 1\nvaria numerus d ← -1\nsit numerus r ← m / d\nnota r");
    assert_eq!(issue, "implicit_store_out_of_range");
}

#[test]
fn bitnot_is_minus_x_minus_one() {
    // rulings 9, 19(b).
    assert_eq!(ok("varia u8 x ← 250\nsit i16 y ← ¬x\nnota y\nnota ¬0"), ["-251", "-1"]);
    assert_eq!(ok("varia u64 a ← 9223372036854775807\nnota ¬a"), ["-9223372036854775808"]);
    let (_, issue, _) = trap("varia u64 a ← 9223372036854775808\nnota ¬a");
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn bitnot_of_u64_max_is_past_the_range() {
    let (out, issue, _) = trap("varia u64 a ← 18446744073709551615\nnota \"a\"\nnota ¬a");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn shifts_are_unmasked() {
    // ruling 10 and the audit's reproductions.
    assert_eq!(
        ok("varia u8 a ← 1\nvaria u8 n ← 63\nsit u64 y ← a ⇐ n\nnota y"),
        ["9223372036854775808"]
    );
    {
        let n = "64";
        let body = format!("varia u8 a ← 1\nvaria u8 n ← {n}\nnota \"a\"\nnota a ⇐ n");
        assert_eq!(trap(&body).0, ["a"]);
    }
    let (_, _, _) = trap("varia u8 a ← 255\nvaria u8 n ← 60\nnota \"a\"\nnota a ⇐ n");
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
            "varia modulus<u32> n ← 300\nsit modulus<u8> w ← n\nnota w\nvaria modulus<u32> m ← 4294967295\nsit modulus<u8> v ← m\nnota v"
        ),
        ["44", "255"]
    );
    assert_eq!(
        ok(
            "varia saturatus<u16> n ← 300\nsit saturatus<u8> w ← n\nnota w\nvaria saturatus<i16> k ← -500\nsit saturatus<i8> u ← k\nnota u"
        ),
        ["255", "-128"]
    );
}

#[test]
fn wrapping_shift_count_is_not_masked() {
    // ruling 10: `1 ⇐ 33` on `wrapping<u32>` is 0.
    assert_eq!(
        ok("varia modulus<u32> a ← 1\nvaria modulus<u32> n ← 33\nsit modulus<u32> s ← a ⇐ n\nnota s\nnota a ⇒ n"),
        ["0", "0"]
    );
}

#[test]
fn true_division_of_words_converts_and_traps_on_zero() {
    // audit item: `÷` on wrapping / saturating operands is real division.
    assert_eq!(
        ok("varia modulus<u8> a ← 7\nvaria modulus<u8> b ← 2\nsit fractus q ← a ÷ b\nnota q"),
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
            "varia numerus a ← 9007199254740993\nvaria fractus f ← 9007199254740992.0\nnota a ≻ f\nnota a ≅ f\nnota a ≇ f"
        ),
        ["verum", "falsum", "verum"]
    );
}

#[test]
fn decimal_float_comparison_is_exact() {
    // ruling 22: 0.1d64 is below the float nearest 0.1.
    assert_eq!(
        ok(
            "varia d64 d ← 0.1\nvaria fractus f ← 0.1\nvaria fractus g ← 0.5\nnota d ≺ f\nnota d ≻ f\nnota d ≅ f\nnota d ≺ g"
        ),
        ["verum", "falsum", "falsum", "verum"]
    );
}

#[test]
fn u64_arithmetic_past_the_range_traps() {
    let (out, issue, _) = trap("varia u64 a ← 18446744073709551615\nvaria u64 b ← 1\nnota \"a\"\nnota a + b");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn explicit_narrowing_stays_recoverable() {
    assert_eq!(ok("varia numerus n ← 300\nsit u8 y ← n ↦ u8 ⊥ 7\nnota y"), ["7"]);
}

#[test]
fn true_division_width_follows_the_operands() {
    // ruling 23: 8- and 16-bit operands divide in f32; 32/64-bit in f64.
    assert_eq!(ok("varia u8 a ← 1\nvaria u8 b ← 3\nnota a ÷ b"), ["0.33333334"]);
    assert_eq!(ok("varia i16 a ← 1\nvaria i32 b ← 3\nnota a ÷ b"), ["0.3333333333333333"]);
    assert_eq!(ok("varia u8 a ← 1\nnota a ÷ 3"), ["0.33333334"]);
    assert_eq!(ok("nota 7 ÷ 2\nnota 1 ÷ 3"), ["3.5", "0.3333333333333333"]);
    // A sub-expression keys on its range type: (u8 + u8) is u16, still f32.
    assert_eq!(
        ok("varia u8 a ← 200\nvaria u8 b ← 100\nvaria u8 c ← 7\nsit _ q ← (a + b) ÷ c\nnota q"),
        ["42.857143"]
    );
    let (out, _, _) = trap("varia u8 a ← 1\nvaria u8 b ← 0\nnota \"a\"\nnota a ÷ b");
    assert_eq!(out, ["a"]);
}

#[test]
fn true_division_word_operand_with_a_literal_converts_from_the_constant_type() {
    // w1: the literal beside a word operand carries the word's HIR type, but its
    // MIR constant is a plain integer; the conversion source is the constant's.
    assert_eq!(ok("varia modulus<u16> w ← 60000\nnota w ÷ 4"), ["15000.0"]);
    assert_eq!(ok("varia modulus<u64> w ← 90\nnota w ÷ 3"), ["30.0"]);
    assert_eq!(ok("varia saturatus<u8> s ← 250\nnota s ÷ 4"), ["62.5"]);
    assert_eq!(
        ok("varia saturatus<u8> s ← 250\nsit fractus q ← (s + 10) ÷ 2\nnota q"),
        ["130.0"]
    );
}

#[test]
fn saturating_chain_division_by_zero_is_an_ordinary_integer_failure() {
    // Ruling 3: saturating arithmetic is exact integer arithmetic, so a zero
    // divisor anywhere in the chain is the integer division failure.
    for body in [
        "varia saturatus<u8> x ← 250\nvaria saturatus<u8> z ← 0\nsit saturatus<u8> r ← x + 10 / z\nnota r",
        "varia saturatus<u8> x ← 250\nvaria saturatus<u8> z ← 0\nsit saturatus<u8> r ← x + x % z\nnota r",
        "varia saturatus<u8> x ← 250\nvaria saturatus<u8> z ← 0\nsit saturatus<u8> r ← (x + 10) / (z + 0)\nnota r",
        "varia saturatus<u8> x ← 250\nvaria saturatus<u8> z ← 0\nnota x / z",
    ] {
        let (_, issue, _) = trap(body);
        assert_eq!(issue, "runner_numerus_division_failed", "{body}");
    }
}

#[test]
fn saturating_u64_intermediates_must_lie_in_the_exact_range() {
    // u9a / ruling 17: every exact value lies in [-2^63, 2^64 - 1], so a
    // chain through 2^64 traps; a chain that stays inside clamps at the store.
    let top = "varia saturatus<u64> top ← 18446744073709551615\n";
    let (_, issue, _) = trap(&format!("{top}sit saturatus<u64> r ← top + 1 - 5\nnota r"));
    assert_eq!(issue, "runner_numerus_overflow");
    assert_eq!(
        ok(&format!("{top}sit saturatus<u64> r ← top - 5 + 3\nnota r")),
        ["18446744073709551613"]
    );
    assert_eq!(
        ok("varia saturatus<u64> z ← 0\nsit saturatus<u64> r ← z - 5 + 3\nnota r"),
        ["0"]
    );
}

const CAP_MAX: &str = "varia numerus a ← 9223372036854775807\nvaria numerus b ← 9223372036854775807\n";

#[test]
fn cap_typed_sum_is_checked_at_argument_and_return_stores() {
    // f1 (ruling 17b): the sum is a legal intermediate but does not fit `i64`.
    let (out, issue, _) = trap(&format!("{CAP_MAX}nota \"a\"\nsit numerus c ← a + b\nnota c"));
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "implicit_store_out_of_range");
    // No slot, no store: the legal intermediate prints.
    assert_eq!(ok(&format!("{CAP_MAX}nota a + b")), ["18446744073709551614"]);
    let id =
        "functio id(numerus x) → numerus { redde x }\nfunctio add(numerus x, numerus y) → numerus { redde x + y }\n";
    let run_module = |main: &str| {
        let session = Session::new(
            Config::default()
                .with_target(Target::HirTypeScript)
                .with_latin_locale(),
        );
        let source = format!("{id}incipit {{\n{CAP_MAX}nota \"a\"\n{main}\n}}\n");
        let mut host = BufferHost::default();
        match run_source(&session, "cap.fab", &source, &mut host) {
            Err(RunSourceError::Stepper(errors)) => (
                host.stdout_lines,
                errors
                    .into_iter()
                    .map(|e| (e.issue, e.message))
                    .collect::<Vec<_>>(),
            ),
            other => panic!("{main}: {other:?}"),
        }
    };
    let (out, errors) = run_module("nota id(a + b)");
    assert_eq!(out, ["a"]);
    assert_eq!(errors[0].0, "implicit_store_out_of_range");
    assert!(errors[0].1.contains("(argument"), "{}", errors[0].1);
    let (out, errors) = run_module("nota add(a, b)");
    assert_eq!(out, ["a"]);
    assert_eq!(errors[0].0, "implicit_store_out_of_range");
    assert!(errors[0].1.contains("(return)"), "{}", errors[0].1);
    // A sum that fits still flows through every store.
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("{id}incipit {{\nnota add(40, 2)\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "cap.fab", &source, &mut host).expect("a fitting sum runs");
    assert_eq!(host.stdout_lines, ["42"]);
}

#[test]
fn bitnot_of_u64_stores_into_int_as_a_checked_narrowing() {
    // f2 (rulings 9, 19a): `¬u` keeps its sign at the cap, so `int` takes it.
    assert_eq!(ok("varia u64 u ← 5\nsit numerus y ← ¬u\nnota y"), ["-6"]);
    let (out, issue, _) = trap("varia u64 u ← 18446744073709551615\nnota \"a\"\nsit numerus y ← ¬u\nnota y");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "runner_numerus_overflow");
}

#[test]
fn decimal_beside_a_nonconstant_integer_compares_exactly() {
    // f3 (ruling 22): no `d64` conversion, so a huge integer does not trap.
    assert_eq!(
        ok("varia d64 d ← 0.5\nvaria numerus i ← 9223372036854775807\nnota d ≺ i\nnota i ≻ d\nnota d ≅ i\nnota i ≇ d"),
        ["verum", "verum", "falsum", "verum"]
    );
    assert_eq!(
        ok("varia d64 d ← 2.0\nvaria numerus i ← 2\nnota d ≅ i\nnota d ≤ i\nnota d ≺ i\nnota i ≻ d"),
        ["verum", "verum", "falsum", "falsum"]
    );
    assert_eq!(
        ok("varia d64 d ← -0.5\nvaria u64 u ← 18446744073709551615\nnota d ≺ u\nnota u ≺ d"),
        ["verum", "falsum"]
    );
}

#[test]
fn approximate_equality_unscales_a_decimal_operand() {
    // f4 (ruling 22): `≈` computes in the float operand's width on true values.
    assert_eq!(
        ok(
            "varia d64 d ← 3.0\nvaria fractus f ← 3.0000000001\nvaria numerus i ← 3\nnota d ≈ f\nnota d ≈ i\nnota i ≈ f\nnota d ≉ f"
        ),
        ["verum", "verum", "verum", "falsum"]
    );
    assert_eq!(
        ok("varia d64 d ← 3.5\nvaria numerus i ← 3\nnota d ≈ i\nnota d ≉ i"),
        ["falsum", "verum"]
    );
}

#[test]
fn word_slot_constants_at_the_64_bit_edges() {
    // lw1 (rulings 12, 21): i64::MIN folds from `-9223372036854775807 - 1`
    // into a saturating slot, and the u64 maximum stores into 64-bit words by
    // declaration, assignment, argument and as an operand beside a word.
    assert_eq!(
        ok("sit saturatus<i64> x ← -9223372036854775807 - 1\nnota x"),
        ["-9223372036854775808"]
    );
    assert_eq!(
        ok("sit saturatus<u64> a ← 18446744073709551615\nsit modulus<u64> b ← 18446744073709551615\nnota a\nnota b"),
        ["18446744073709551615", "18446744073709551615"]
    );
    assert_eq!(
        ok("varia saturatus<u64> s ← 0\ns ← 18446744073709551615\nnota s"),
        ["18446744073709551615"]
    );
    assert_eq!(
        ok(
            "sit modulus<u64> m ← 5\nsit modulus<u64> p ← m + 18446744073709551615\nsit modulus<u64> q ← 18446744073709551615 + m\nnota p\nnota q"
        ),
        ["4", "4"]
    );
}

#[test]
fn word_parameter_takes_a_constant_argument() {
    let functions = "functio f(saturatus<u64> a) → saturatus<u64> { redde a }\nfunctio g(modulus<u8> a) → modulus<u8> { redde a }\n";
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let program = format!("{functions}incipit {{\nnota f(18446744073709551615)\nnota g(5)\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "exact.fab", &program, &mut host).expect("runs");
    assert_eq!(host.stdout_lines, ["18446744073709551615", "5"]);
}

#[test]
fn negated_i64_min_constant_fits_every_64_bit_slot() {
    // lw2 (rulings 12, 21): `-2^63` is an exact constant that fits `i64`; only
    // the magnitude is out of range. One below it is a compile error in a
    // trapping slot and follows the slot's policy in a saturating one.
    assert_eq!(
        ok(
            "sit saturatus<i64> x ← -9223372036854775808\nvaria saturatus<i64> y ← -9223372036854775808\nsit i64 z ← -9223372036854775808\nsit numerus w ← -9223372036854775808\nnota x\nnota y\nnota z\nnota w"
        ),
        ["-9223372036854775808"; 4]
    );
    assert_eq!(
        ok("sit saturatus<i64> x ← -9223372036854775809\nnota x"),
        ["-9223372036854775808"]
    );
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = "incipit {\nsit numerus z ← -9223372036854775809\nnota z\n}\n";
    let mut host = BufferHost::default();
    assert!(run_source(&session, "exact.fab", source, &mut host).is_err());
}

#[test]
fn summa_at_the_cap_is_a_checked_store() {
    // f1s (ruling 21): a `summa` sits at the 64-bit cap, so returning it as
    // `int` checks the store; a small sum returns fine.
    let functions =
        "functio total(tensor<i64, [3]> a) → numerus {\n  redde reducta via summa ex a sit s {\n    redde s\n  }\n}\n";
    let run_total = |values: &str| {
        let session = Session::new(
            Config::default()
                .with_target(Target::HirTypeScript)
                .with_latin_locale(),
        );
        let source = format!(
            "{functions}incipit {{\nsit lista<i64> v ← [{values}]\nnota \"a\"\nnota total(v ↦ tensor<i64, [3]>)\n}}\n"
        );
        let mut host = BufferHost::default();
        let result = run_source(&session, "reducta.fab", &source, &mut host);
        (host.stdout_lines, result)
    };
    let (out, result) = run_total("1, 2, 0");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, ["a", "3"]);
    let (out, result) = run_total("9223372036854775807, 9223372036854775807, 0");
    let Err(RunSourceError::Stepper(errors)) = result else {
        panic!("no trap: {result:?}");
    };
    assert_eq!(out, ["a"]);
    assert_eq!(errors[0].issue, "implicit_store_out_of_range");
}

#[test]
fn reducta_summa_widens_narrow_integer_terms_before_accumulating() {
    // Ruling 21 types an integer sum at the 64-bit cap before the final store.
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = "functio total(tensor<i8, [2]> a) → numerus {\n  redde reducta via summa ex a\n}\n\
                  incipit {\nsit tensor<i8, [2]> a ← [100, 100] ↦ tensor<i8, [2]>\nnota total(a)\n}\n";
    let mut host = BufferHost::default();
    let result = run_source(&session, "reducta.fab", source, &mut host);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(host.stdout_lines, ["200"]);
}

#[test]
fn grouped_reducta_summa_widens_narrow_integer_terms_before_accumulating() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = "functio grouped(tensor<i8, [2]> values, tensor<numerus, [2]> keys) → tensor<i64, [1]> {\n  redde reducta via summa ex values apud [i] pro [keys[i]]\n}\n\
                  incipit {\nsit tensor<i8, [2]> values ← [100, 100] ↦ tensor<i8, [2]>\nsit tensor<numerus, [2]> keys ← [0, 0] ↦ tensor<numerus, [2]>\nnota grouped(values, keys)\n}\n";
    let mut host = BufferHost::default();
    let result = run_source(&session, "reducta-group.fab", source, &mut host);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(host.stdout_lines, ["[200]"]);
}

#[test]
fn summa_accumulates_exactly_and_checks_at_the_store() {
    // sm1: the running total is exact (2^64 - 2 is inside the exact range);
    // only the store of the result into its slot applies the slot policy.
    let functions =
        "functio total(tensor<i64, [3]> a) → numerus {\n  redde reducta via summa ex a sit s {\n    redde s\n  }\n}\n";
    let run_total = |values: &str| {
        let session = Session::new(
            Config::default()
                .with_target(Target::HirTypeScript)
                .with_latin_locale(),
        );
        let source =
            format!("{functions}incipit {{\nsit lista<i64> v ← [{values}]\nnota total(v ↦ tensor<i64, [3]>)\n}}\n");
        let mut host = BufferHost::default();
        let result = run_source(&session, "reducta.fab", &source, &mut host);
        (host.stdout_lines, result)
    };
    let max = "9223372036854775807";
    let (out, result) = run_total(&format!("{max}, {max}, -{max}"));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, [max]);
    let (out, result) = run_total(&format!("{max}, {max}, {max}"));
    let Err(RunSourceError::Stepper(errors)) = result else {
        panic!("no trap: {out:?} {result:?}");
    };
    assert_eq!(errors[0].issue, "runner_numerus_overflow");
}

#[test]
fn saturating_unary_negation_is_exact_and_clamps_at_the_store() {
    // sx5 / ruling 3, 19: `-s` over an unsigned saturating leaf is exact.
    let s = "varia saturatus<u8> s ← 5\n";
    assert_eq!(ok(&format!("{s}nota -s")), ["-5"]);
    assert_eq!(ok(&format!("{s}sit saturatus<u8> t ← -s\nnota t")), ["0"]);
}

#[test]
fn saturating_bitwise_operators_are_exact_and_clamp_at_the_store() {
    // sx6 / ruling 19: `∧ ∨ ⊻ ¬ ⇐ ⇒` are exact over integers; only the store clamps.
    let s = "varia saturatus<u8> s ← 200\n";
    assert_eq!(ok(&format!("{s}nota ¬s")), ["-201"]);
    assert_eq!(ok(&format!("{s}sit saturatus<u8> t ← ¬s\nnota t")), ["0"]);
    assert_eq!(ok(&format!("{s}nota s ∨ 0x100")), ["456"]);
    assert_eq!(ok(&format!("{s}sit saturatus<u8> t ← s ∨ 0x100\nnota t")), ["255"]);
    assert_eq!(ok(&format!("{s}nota s ∧ 0x0f")), ["8"]);
    assert_eq!(ok(&format!("{s}nota s ⊻ 0x0f")), ["199"]);
    assert_eq!(ok(&format!("{s}nota s ⇐ 2")), ["800"]);
    assert_eq!(ok(&format!("{s}sit saturatus<u8> t ← s ⇐ 2\nnota t")), ["255"]);
    assert_eq!(ok(&format!("{s}nota s ⇒ 2")), ["50"]);
}

// ── ss1: numeric method twins are their operators ─────────────────────────

#[test]
fn value_twin_is_exact_and_checked_at_the_store() {
    assert_eq!(ok("sit u8 x ← 200\nnota x.addita(100)"), ["300"]);
    let (out, issue, message) = trap("sit u8 x ← 200\nnota \"a\"\nsit u8 y ← x.addita(100)\nnota y");
    assert_eq!(out, ["a"]);
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(
        message.contains("300 does not fit in `u8`") && message.contains("`y`"),
        "{message}"
    );
}

#[test]
fn in_place_twin_is_a_store_into_the_place() {
    assert_eq!(ok("varia u8 x ← 100\nx.adde(100)\nnota x"), ["200"]);
    for body in [
        "varia u8 x ← 200\nnota \"a\"\nx.adde(100)\nnota x",
        "varia u8 x ← 200\nnota \"a\"\nx.multiplica(2)\nnota x",
        "varia u8 x ← 200\nnota \"a\"\nx.sinistra(1)\nnota x",
    ] {
        let (out, issue, message) = trap(body);
        assert_eq!(out, ["a"], "{body}");
        assert_eq!(issue, "implicit_store_out_of_range", "{body}");
        assert!(message.contains("`x`"), "{message}");
    }
}

#[test]
fn in_place_twin_through_an_index_place() {
    assert_eq!(
        ok("varia _ xs ← [1, 2, 3] ∷ lista<i32>\nxs[1].multiplica(10)\nnota xs[1]"),
        ["20"]
    );
}

#[test]
fn unary_twins_follow_the_range_rule() {
    assert_eq!(ok("sit i8 n ← -128\nnota n.negativa()"), ["128"]);
    let (_, issue, _) = trap("sit i8 n ← -128\nsit i8 m ← n.negativa()\nnota m");
    assert_eq!(issue, "implicit_store_out_of_range");
}

#[test]
fn word_twins_keep_the_word_semantics() {
    assert_eq!(ok("varia modulus<u8> w ← 200\nw.adde(100)\nnota w"), ["44"]);
    // The twin means the operator: exact until it lands in a slot (§8).
    assert_eq!(ok("sit modulus<u8> w ← 200\nnota w.addita(100)"), ["300"]);
    assert_eq!(ok("sit modulus<u8> w ← 200\nsit modulus<u8> s ← w.addita(100)\nnota s"), ["44"]);
}

#[test]
fn absolutum_is_exact_for_sized_integers() {
    assert_eq!(ok("sit i8 n ← -128\nnota n.absolutum()"), ["128"]);
    assert_eq!(ok("sit i8 n ← -128\nsit u8 m ← n.absolutum()\nnota m"), ["128"]);
    let (_, issue, _) = trap("sit i8 n ← -128\nsit i8 m ← n.absolutum()\nnota m");
    assert_eq!(issue, "implicit_store_out_of_range");
    // `|i64::MIN|` is 2^63, a valid exact value.
    assert_eq!(
        ok("sit numerus m ← -9223372036854775807 - 1\nnota m.absolutum()"),
        ["9223372036854775808"]
    );
}

#[test]
fn potentia_is_exact_and_range_typed() {
    assert_eq!(ok("sit i32 p ← 100000\nnota p.potentia(2)"), ["10000000000"]);
    let (_, issue, message) = trap("sit i32 p ← 100000\nsit i32 q ← p.potentia(2)\nnota q");
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("10000000000"), "{message}");
    assert_eq!(ok("sit u8 u ← 3\nsit u8 q ← u.potentia(3)\nnota q"), ["27"]);
}

fn run_module(source: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "place.fab", source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{source}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

const PLACE_HELPER: &str = "functio idx(numerus n) → numerus {\n    nota \"idx\"\n    redde n\n}\n";

#[test]
fn in_place_twin_evaluates_an_effectful_place_once() {
    let source = format!(
        "{PLACE_HELPER}incipit {{\nvaria _ xs ← [10, 20, 30] ∷ lista<i32>\nxs[idx(1)].adde(1)\nnota xs[1]\n}}\n"
    );
    let (out, errors) = run_module(&source);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["idx", "21"]);
}

#[test]
fn in_place_twin_evaluates_each_effectful_index_once_in_order() {
    let source = format!(
        "{PLACE_HELPER}incipit {{\nvaria lista<lista<numerus>> g ← [[1, 2], [3, 4]]\ng[idx(1)][idx(0)].adde(10)\nnota g[1][0]\n}}\n"
    );
    let (out, errors) = run_module(&source);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["idx", "idx", "13"]);
}

#[test]
fn in_place_twin_evaluates_more_than_sixteen_effectful_indices_once() {
    // ss1d: the temporaries are not a fixed pool; a place with 20 effectful
    // indices desugars and evaluates each once.
    let depth = 20;
    let ty = format!("{}numerus{}", "lista<".repeat(depth), ">".repeat(depth));
    let literal = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let place = "[idx(0)]".repeat(depth);
    let source = format!("{PLACE_HELPER}incipit {{\nvaria {ty} g ← {literal}\ng{place}.adde(1)\nnota g{place}\n}}\n");
    let (out, errors) = run_module(&source);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out.len(), 2 * depth + 1, "{out:?}");
    assert!(out[..2 * depth].iter().all(|line| line == "idx"), "{out:?}");
    assert_eq!(out.last().map(String::as_str), Some("2"));
}

#[test]
fn in_place_twin_on_an_effectful_place_traps_at_the_store() {
    let source = format!(
        "{PLACE_HELPER}incipit {{\nvaria _ xs ← [10, 20, 30] ∷ lista<u8>\nxs[idx(1)].multiplica(1000)\nnota xs[1]\n}}\n"
    );
    let (out, errors) = run_module(&source);
    assert_eq!(out, ["idx"]);
    assert_eq!(errors[0].0, "implicit_store_out_of_range", "{errors:?}");
}

#[test]
fn in_place_twin_yields_no_value() {
    // ss1a: the imperative form mutates and returns nothing, so it is not an
    // initializer.
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = "incipit {\nvaria numerus x ← 1\nsit numerus y ← x.adde(1)\nnota y\n}\n";
    let mut host = BufferHost::default();
    let Err(RunSourceError::Frontend(diagnostics)) = run_source(&session, "value.fab", source, &mut host) else {
        panic!("using an in-place twin as a value must not analyze");
    };
    let text = format!("{diagnostics:?}");
    assert!(text.contains("expression_type_mismatch"), "{text}");
    assert!(text.contains("initializer_annotation_mismatch"), "{text}");
}

#[test]
fn list_sum_is_typed_at_the_cap_and_exact() {
    // ss4 (rulings 17, 21): the `.summa()` method is typed like the construct.
    // Partial sums are exact; only the store applies the slot policy.
    assert_eq!(ok("sit lista<i8> a ← [100, 100, -100]\nnota a.summa()"), ["100"]);
    assert_eq!(
        ok("sit lista<numerus> a ← [9223372036854775807, 1, -1]\nnota a.summa()"),
        ["9223372036854775807"]
    );
    assert_eq!(ok("sit lista<u8> c ← [200, 100]\nnota c.summa()"), ["300"]);
    assert_eq!(
        ok("sit lista<i8> a ← [100, 100, -100]\nvaria i8 t ← a.summa()\nnota t"),
        ["100"]
    );
    let (out, _, message) = trap("sit lista<u8> c ← [200, 100]\nvaria u8 s ← c.summa()\nnota s");
    assert!(out.is_empty());
    assert!(message.contains("300 does not fit in `u8`"), "{message}");
}

#[test]
fn list_sum_at_the_cap_is_a_checked_store() {
    // The exact total above `i64::MAX` is a cap-typed value; storing it into
    // `int` is the checked store.
    let (_, issue, _) =
        trap("sit lista<numerus> a ← [9223372036854775807, 9223372036854775807]\nsit numerus s ← a.summa()\nnota s");
    assert_eq!(issue, "implicit_store_out_of_range");
}

#[test]
fn tensor_matmul_dot_products_are_exact_and_store_per_element() {
    // Partial sum 200 leaves `i8`; the finished cell 0 fits and is stored.
    let setup = "sit lista<i8> l_data ← [100, 100, 100]\nsit tensor<i8, [1, 3]> l ← l_data ↦ tensor<i8, [1, 3]>\n";
    let fits = format!(
        "{setup}sit lista<i8> r_data ← [1, 1, -2]\nsit tensor<i8, [3, 1]> r ← r_data ↦ tensor<i8, [3, 1]>\nsit tensor<i8, [1, 1]> p ← l.matmul(r)\nnota p.summa()"
    );
    assert_eq!(ok(&fits), ["0"]);
    let too_big = format!(
        "{setup}sit lista<i8> r_data ← [1, 1, 1]\nsit tensor<i8, [3, 1]> r ← r_data ↦ tensor<i8, [3, 1]>\nsit tensor<i8, [1, 1]> p ← l.matmul(r)\nnota p.summa()"
    );
    let (_, issue, message) = trap(&too_big);
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("300 does not fit in `i8`"), "{message}");
}

#[test]
fn tensor_elementwise_stores_each_element_under_the_element_type() {
    // ts1 (rulings 17, 19): each element is exact and stored per element.
    let setup = "sit tensor<i8, [3]> a ← [100, 100, -100] ↦ tensor<i8, [3]>\n";
    // `i8 + i8` takes the scalar range rule → `i16`; the checked `i8` lane store
    // traps on the widened element (operator ruling 2026-10-15, need).
    let sum = format!(
        "{setup}sit tensor<i8, [3]> b ← [100, 1, -100] ↦ tensor<i8, [3]>\nsit tensor<i16, [3]> wide ← a + b\nvaria tensor<i8, [3]> r ← [0, 0, 0] ↦ tensor<i8, [3]>\nr[0] ← wide[0]\nnota r.summa()"
    );
    let (out, issue, message) = trap(&sum);
    assert!(out.is_empty());
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("200 does not fit in `i8`"), "{message}");
    let prod = format!(
        "{setup}sit tensor<i8, [3]> b ← [1, 1, 1] ↦ tensor<i8, [3]>\nsit tensor<i8, [3]> s ← a ⊙ b\nnota s.summa()"
    );
    assert_eq!(ok(&prod), ["100"]);
}
