//! inf track U4: the unbounded integer `inf` end to end through the MIR runner
//! (the oracle). Each test names the inf-delivery ruling it pins; the probes
//! are the delivery spec's U4 done-when set.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> (Vec<String>, Vec<(String, String)>) {
    run_items("", body)
}

/// `items` are top-level declarations; `body` is the `incipit` block.
fn run_items(items: &str, body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let source = format!("{items}\nincipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "inf.fab", &source, &mut host) {
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

const TWO_64: &str = "18446744073709551616";

#[test]
fn factorial_of_30_runs_in_an_inf_loop() {
    // ruling 36: `*` never traps on size.
    assert_eq!(
        ok("varia inf f ← 1\nvaria numerus i ← 1\ndum i ≤ 30 {\n f ← f * i\n i ← i + 1\n}\nnota f"),
        ["265252859812191058636308480000000"]
    );
}

#[test]
fn shifts_are_exact_and_unmasked() {
    // ruling 36: `x ⇐ n` is `x·2ⁿ`, `x ⇒ n` is `⌊x/2ⁿ⌋`.
    assert_eq!(
        ok("sit inf one ← 1\nsit inf p ← one ⇐ 200\nnota p"),
        ["1606938044258990275541962092341162602522202993782792835301376"]
    );
    assert_eq!(
        ok("sit inf two ← 2\nnota two ⇐ 200"),
        ["3213876088517980551083924184682325205044405987565585670602752"]
    );
    assert_eq!(ok("sit inf x ← -1 ⇐ 100\nnota x ⇒ 98"), ["-4"]);
    assert_eq!(
        ok(&format!("sit inf x ← {TWO_64}\nnota x ⇒ 64\nnota x ⇒ 65\nnota x ⇒ 1000")),
        ["1", "0", "0"]
    );
    assert_eq!(ok("sit inf x ← -5\nnota x ⇒ 100"), ["-1"]);
}

#[test]
fn negative_shift_count_is_the_runtime_trap() {
    let (out, _, message) = trap("sit inf x ← 5\nvaria numerus n ← -1\nnota \"a\"\nnota x ⇐ n");
    assert_eq!(out, ["a"]);
    assert!(message.contains("negative shift count"), "{message}");
}

#[test]
fn division_and_remainder_floor_on_inf() {
    // rulings 14, 29, 36.
    assert_eq!(
        ok("sit inf a ← -7\nnota a / 2\nnota a % 2\nnota 7 / a\nnota 7 % a"),
        ["-4", "1", "-1", "0"]
    );
    assert_eq!(
        ok("sit inf a ← -100000000000000000000000000000000000000000\nnota a / 7\nnota a % 7"),
        ["-14285714285714285714285714285714285714286", "2"]
    );
}

#[test]
fn a_zero_divisor_is_the_division_trap() {
    let (_, _, message) = trap("sit inf a ← 5\nsit inf z ← 0\nnota a / z");
    assert!(message.contains("numerus division failed"), "{message}");
    let (_, _, message) = trap("sit inf a ← 5\nsit inf z ← 0\nnota a % z");
    assert!(message.contains("numerus division failed"), "{message}");
}

#[test]
fn unary_minus_and_bitnot_are_exact() {
    // ruling 36: `¬x = -x - 1` on infinite two's complement.
    assert_eq!(ok("sit inf x ← -7\nnota ¬x\nnota -x"), ["6", "7"]);
    assert_eq!(
        ok(&format!("sit inf x ← {TWO_64}\nnota ¬x\nnota -x")),
        ["-18446744073709551617", "-18446744073709551616"]
    );
}

#[test]
fn bitwise_ops_act_on_infinite_two_complement() {
    // ruling 19(b) / 36: `x ∧ -1 = x`, sign extension is unbounded.
    assert_eq!(
        ok("sit inf x ← -7\nnota x ∧ -1\nnota x ∧ 255\nnota x ∨ 8\nnota x ⊻ -1"),
        ["-7", "249", "-7", "6"]
    );
    assert_eq!(
        ok(&format!("sit inf x ← {TWO_64}\nnota x ∧ -1\nnota x ∨ 1\nnota x ⊻ x")),
        ["18446744073709551616", "18446744073709551617", "0"]
    );
}

#[test]
fn arithmetic_crosses_the_64_bit_cap_without_trapping() {
    // ruling 36: `u64::MAX + 1` is exact in `inf`.
    assert_eq!(
        ok("sit inf m ← 18446744073709551615\nnota m + 1\nnota m * m\nnota -m - m"),
        [
            "18446744073709551616",
            "340282366920938463426481119284349108225",
            "-36893488147419103230"
        ]
    );
}

#[test]
fn a_bounded_operand_widens_into_an_inf_expression() {
    // ruling 35: `inf` is contagious only from a typed `inf` operand.
    assert_eq!(
        ok("sit inf x ← 18446744073709551615\nsit u64 u ← 5\nnota x + u\nnota u + x"),
        ["18446744073709551620", "18446744073709551620"]
    );
}

#[test]
fn a_bounded_value_widens_into_every_inf_slot_exactly() {
    // inf-wn: the widening node is the identity on the runner's canonical
    // carrier, so a `u64` beyond `i64` keeps its value at every site.
    let items = "functio take(inf x) → inf {\n redde x\n}\n\
                 functio give(u64 v) → inf {\n redde v\n}\n\
                 genus Capsa {\n sit inf f\n}";
    let (out, errors) = run_items(
        items,
        "sit u64 u ← 18446744073709551615\nvaria inf w ← 0\nw ← u\nnota w + 1\n\
         nota take(u) + 1\nnota give(u) + 1\nsit Capsa c ← Capsa { f = u }\nnota c.f + 1\n\
         sit lista<inf> xs ← [u, w]\nnota xs[0] + 1",
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["18446744073709551616"; 5]);
}

#[test]
fn bounded_arithmetic_stored_into_an_inf_slot_keeps_the_64_bit_cap() {
    // Ruling 35: an expression over bounded operands keeps ruling 13's cap, so
    // `u + 1` at `u64::MAX` traps; the store into the `inf` slot only widens
    // the (in-range) result. `(u ↦ inf) + 1` is the opt-in exact form.
    let (_, issue, message) = trap("sit u64 u ← 18446744073709551615\nsit inf w ← u + 1\nnota w");
    assert_eq!(issue, "runner_numerus_overflow", "{message}");
    assert_eq!(
        ok("sit u64 u ← 18446744073709551615\nsit inf w ← (u ↦ inf) + 1\nnota w"),
        ["18446744073709551616"]
    );
}

#[test]
fn potentia_is_exact_and_a_negative_exponent_traps() {
    // rulings 28, 36.
    assert_eq!(
        ok("sit inf b ← 3\nnota b.potentia(100)"),
        ["515377520732011331036461129765621272702107522001"]
    );
    assert_eq!(ok("sit inf z ← 0\nnota z.potentia(0)\nnota z.potentia(5)"), ["1", "0"]);
    assert_eq!(
        ok("sit inf b ← -2\nnota b.potentia(63)\nnota b.potentia(64)"),
        ["-9223372036854775808", "18446744073709551616"]
    );
}

#[test]
#[should_panic(expected = "negative exponent")]
fn a_negative_potentia_exponent_aborts_like_the_division_class() {
    // ruling 28: the trap of a zero divisor's class, never `⇥`.
    let _ = run("sit inf b ← 3\nvaria numerus e ← -1\nnota b.potentia(e)");
}

#[test]
fn comparisons_between_inf_and_every_family_are_exact() {
    // ruling 37 (and ruling 22's rule for the other pairs): never rounded.
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit u64 u ← 18446744073709551615\nnota big ≻ u\nnota u ≺ big\nnota big ≥ u\nnota big ≡ big"
        )),
        ["verum", "verum", "verum", "verum"]
    );
    // f64: 2^64 is exactly representable, 2^64 + 1 is not — the inf is not rounded.
    assert_eq!(
        ok(&format!(
            "sit inf a ← {TWO_64}\nsit inf b ← 18446744073709551617\nsit fractus f ← 1.8446744073709552e19\nnota a ≅ f\nnota b ≅ f\nnota b ≻ f\nnota a ≥ f"
        )),
        ["verum", "falsum", "verum", "verum"]
    );
    assert_eq!(
        ok("sit inf h ← 10 ⇐ 1100\nsit fractus f ← 1.0e300\nnota h ≻ f\nnota -h ≺ f\nsit inf n ← -h\nnota n ≺ f"),
        ["verum", "verum", "verum"]
    );
    assert_eq!(
        ok("sit inf five ← 5\nsit d64 d ← 1.5\nnota five ≻ d\nnota d ≺ five\nnota five ≤ d"),
        ["verum", "verum", "falsum"]
    );
    assert_eq!(
        ok(&format!("sit inf big ← {TWO_64}\nsit d64 d ← 1.5\nnota big ≻ d\nnota -big ≺ d")),
        ["verum", "verum"]
    );
}

#[test]
fn integer_division_to_a_float_is_f64() {
    // ruling 36: `÷` on `inf` is true division in f64; an integer zero divisor traps.
    assert_eq!(ok("sit inf a ← 7\nnota a ÷ 2"), ["3.5"]);
    assert_eq!(ok(&format!("sit inf a ← {TWO_64}\nnota a ÷ 2")), ["9223372036854775808.0"]);
    let (_, _, message) = trap("sit inf a ← 7\nsit inf z ← 0\nnota a ÷ z");
    assert!(message.contains("numerus division failed"), "{message}");
}

#[test]
fn increment_and_decrement_cross_the_cap() {
    // ruling 36: `↑` / `↓` on an `inf` place never trap.
    assert_eq!(
        ok("varia inf x ← 18446744073709551615\nx ↑\nnota x\nx ↓\nx ↓\nnota x"),
        ["18446744073709551616", "18446744073709551614"]
    );
}

#[test]
fn an_inf_value_stores_into_a_bounded_slot_only_when_it_fits() {
    // ruling 38: an implicit checked narrowing, never `⇥`, with the store identity.
    let (_, issue, message) = trap("sit inf t ← 300\nvaria u8 b ← 1\nb ← t");
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(
        message.contains("300 does not fit in `u8`") && message.contains("`b`"),
        "{message}"
    );
    let (_, issue, message) = trap(&format!("sit inf t ← {TWO_64} ⇐ 4\nsit u8 b ← t\nnota b"));
    assert_eq!(issue, "implicit_store_out_of_range");
    assert!(message.contains("does not fit in `u8`") && message.contains("`b`"), "{message}");
    assert_eq!(ok("sit inf t ← 200\nsit u8 b ← t\nnota b"), ["200"]);
    // A store into an `inf` slot is total.
    assert_eq!(ok("sit u8 b ← 200\nsit inf t ← b\nnota t + 100"), ["300"]);
}

#[test]
fn conversions_out_of_inf_narrow_checked_or_reduce_or_clamp() {
    // ruling 40.
    let src = format!(
        "sit inf big ← {TWO_64}\nsit inf mid ← 200\nsit inf neg ← -1\n\
         nota mid ↦ u8 ⊥ 7\nnota big ↦ u8 ⊥ 7\nnota big ↦ u64 ⊥ 9\n\
         nota neg ↦ u8 ⊥ 8\nnota big ↦ numerus ⊥ 5\nnota mid ↦ i8 ⊥ -1\n\
         sit inf sum ← big + 5\nnota sum ↦ modulus<u8>\nnota big ↦ saturatus<u8>\nnota neg ↦ saturatus<u8>\nnota mid ↦ modulus<i8>"
    );
    assert_eq!(ok(&src), ["200", "7", "9", "8", "5", "-1", "5", "255", "0", "-56"]);
}

#[test]
fn conversions_to_floats_round_nearest_even_and_overflow_to_infinity() {
    // ruling 40 (operator ruling: beyond the finite range is ±∞).
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit inf e ← 10 ⇐ 1100\nnota big ↦ fractus\nnota e ↦ fractus\nsit inf ne ← -e\nnota ne ↦ fractus"
        )),
        ["18446744073709551616.0", "inf", "-inf"]
    );
    // nearest-even: 2^53 + 1 ties to 2^53; 2^53 + 3 ties to 2^53 + 4.
    assert_eq!(
        ok("sit inf a ← 9007199254740993\nsit inf b ← 9007199254740995\nnota a ↦ fractus\nnota b ↦ fractus"),
        ["9007199254740992.0", "9007199254740996.0"]
    );
}

#[test]
fn conversions_from_floats_truncate_and_nan_and_infinity_fail() {
    // ruling 40.
    assert_eq!(
        ok("sit fractus x ← 1.0e25\nsit fractus y ← -2.9\nnota x ↦ inf via Trunc ⊥ 0\nnota y ↦ inf via Trunc ⊥ 99"),
        ["10000000000000000905969664", "-2"]
    );
    assert_eq!(
        ok(
            "sit fractus nan ← 0.0 ÷ 0.0\nsit fractus big ← ∞\nnota nan ↦ inf via Trunc ⊥ 42\nnota big ↦ inf via Trunc ⊥ 43\nnota -big ↦ inf via Trunc ⊥ 44"
        ),
        ["42", "43", "44"]
    );
}

#[test]
fn conversions_with_d64_go_through_the_d64_rules() {
    // ruling 40 (amended by need): `d64 → inf` is the integer reduction and names its rounding (`via HalfEven`); `inf → d64` is range-checked.
    assert_eq!(
        ok(&format!(
            "sit d64 d ← 12.75\nsit inf mid ← 200\nsit inf big ← {TWO_64}\nnota d ↦ inf via HalfEven ⊥ 0\nnota mid ↦ d64 ⊥ 0.0\nnota big ↦ d64 ⊥ 1.5"
        )),
        ["13", "200", "1.5"]
    );
}

#[test]
fn text_parses_any_length_and_formats_in_every_radix() {
    // ruling 40.
    assert_eq!(
        ok(
            "nota \"123456789012345678901234567890\" ↦ inf ⊥ 0\nnota \"-99999999999999999999\" ↦ inf ⊥ 0\nnota \"12x\" ↦ inf ⊥ 77\nnota \"\" ↦ inf ⊥ 78"
        ),
        ["123456789012345678901234567890", "-99999999999999999999", "77", "78"]
    );
    assert_eq!(
        ok(
            "nota \"ffffffffffffffffffff\" ↦ inf via Hex ⊥ 0\nnota \"101\" ↦ inf via Bin ⊥ 0\nnota \"-17\" ↦ inf via Oct ⊥ 0"
        ),
        ["1208925819614629174706175", "5", "-15"]
    );
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit inf neg ← -1\nnota big ↦ textus\nnota neg ↦ textus\nsit ascii<magnitudo 17> h ← big ↦ ascii<magnitudo 17> via Hex\nnota h"
        )),
        ["18446744073709551616", "-1", "10000000000000000"]
    );
}

#[test]
fn octeti_via_endian_is_minimal_two_complement() {
    // ruling 40: Java `toByteArray` semantics, and the exact inverse on decode.
    assert_eq!(
        ok(
            "sit inf z ← 0\nsit inf p ← 128\nsit inf n ← -129\nsit lista<b8> a ← z ↦ lista<b8> via Be\nsit lista<b8> b ← p ↦ lista<b8> via Be\nsit lista<b8> c ← n ↦ lista<b8> via Be\nnota a.longitudo()\nnota b.longitudo()\nnota c.longitudo()\nnota a[0] ↦ u8\nnota b[0] ↦ u8\nnota b[1] ↦ u8\nnota c[0] ↦ u8\nnota c[1] ↦ u8"
        ),
        ["1", "2", "2", "0", "0", "128", "255", "127"]
    );
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit lista<b8> le ← big ↦ lista<b8> via Le\nnota le.longitudo()\nnota le[8] ↦ u8\nsit inf back ← le ↦ inf via Le ⊥ 0\nnota back\nsit lista<b8> be ← big ↦ lista<b8> via Be\nsit inf back2 ← be ↦ inf via Be ⊥ 0\nnota back2"
        )),
        ["9", "1", "18446744073709551616", "18446744073709551616"]
    );
}

#[test]
fn display_print_interpolation_and_composites() {
    // ruling 42.
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit inf neg ← -{TWO_64}\nnota big\nnota neg\nnota \"x§y\"(big)\nsit lista<inf> xs ← [big, neg, 1]\nnota xs"
        )),
        [
            "18446744073709551616",
            "-18446744073709551616",
            "x18446744073709551616y",
            "[18446744073709551616, -18446744073709551616, 1]",
        ]
    );
}

#[test]
fn format_specs_apply_to_inf_like_int() {
    // ruling 42: `¶` integer specs over the exact digits.
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit inf neg ← -255\nnota big ¶ \"x\"\nnota neg ¶ \"x\"\nnota big ¶ \"b\"\nnota neg ¶ \"+>12\"\nnota big ¶ \">30\"\nnota big ¶ \"030\"\nnota big ¶ \".2\"\nnota big ¶ \"e\""
        )),
        [
            "10000000000000000",
            "-ff",
            "10000000000000000000000000000000000000000000000000000000000000000",
            "++++++++-255",
            "          18446744073709551616",
            "000000000018446744073709551616",
            "18446744073709551616.00",
            "1.8446744073709552e19",
        ]
    );
}

#[test]
fn an_inf_key_hashes_by_value_in_tabula_and_copia() {
    // ruling 37: `tabula<inf, textus>` works, and 2^64 collides with itself only.
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit inf nbig ← -big\nvaria tabula<inf, textus> names ← vacua\nnames[big] ← \"two-to-64\"\nnames[5] ← \"five\"\nnames[nbig] ← \"neg\"\nnames[big + 0] ← \"again\"\nnota names[big]\nnota names[5]\nnota names[nbig]\nnota names.longitudo()"
        )),
        ["again", "five", "neg", "3"]
    );
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nvaria copia<inf> seen ← vacua\nseen.adde(big)\nseen.adde(big + 0)\nseen.adde(5)\nseen.adde(5)\nnota seen.longitudo()\nnota seen.habet(big)\nnota seen.habet(6)"
        )),
        ["2", "verum", "falsum"]
    );
}

#[test]
fn elige_matches_inf_subjects_by_value() {
    // ruling 43: `elige` on an `inf` subject, beyond and within the 64-bit carriers.
    assert_eq!(
        ok(&format!(
            "sit inf k ← {TWO_64} + 1\nelige k {{\n casu 18446744073709551617 {{ nota \"hit\" }}\n casu 18446744073709551616 {{ nota \"miss\" }}\n ceterum {{ nota \"other\" }}\n}}\nsit inf e ← 7\nelige e {{\n casu 5 {{ nota \"five\" }}\n casu 7 {{ nota \"seven\" }}\n ceterum {{ nota \"other\" }}\n}}\nsit inf n ← -{TWO_64}\nelige n {{\n casu -18446744073709551616 {{ nota \"neg\" }}\n ceterum {{ nota \"other\" }}\n}}"
        )),
        ["hit", "seven", "neg"]
    );
}

#[test]
fn lista_of_inf_sums_exactly_and_orders_totally() {
    // ruling 43: `summa` over `lista<inf>`; minimus/maximus/absolutum/signum exact.
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit lista<inf> xs ← [big, 2, -big, 7]\nnota xs.summa()\nsit lista<inf> ys ← [big, big]\nnota ys.summa()"
        )),
        ["9", "36893488147419103232"]
    );
    assert_eq!(
        ok(&format!(
            "sit inf big ← {TWO_64}\nsit inf small ← 5\nsit inf nbig ← -big\nnota big.maximus(small)\nnota big.minimus(small)\nnota nbig.absolutum()\nnota nbig.signum()\nnota small.signum()"
        )),
        ["18446744073709551616", "5", "18446744073709551616", "-1", "1"]
    );
}

#[test]
fn genus_fields_and_functions_carry_inf() {
    // ruling 43: fields, calls and returns; ruling 42: D2.8 composite display.
    let (out, errors) = run_items(
        "genus Capsa {\n sit inf a\n sit inf b\n}\nfunctio duplica(inf x) → inf {\n redde x + x\n}\n",
        &format!("sit inf big ← {TWO_64}\nsit Capsa c ← Capsa {{ a = big, b = duplica(big) }}\nnota c\nnota c.b - c.a"),
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        out,
        [
            "Capsa { a = 18446744073709551616, b = 36893488147419103232 }",
            "18446744073709551616"
        ]
    );
}

#[test]
fn fuzzy_equality_is_the_integer_tolerance_between_inf_operands_and_float_width_against_a_float() {
    // ruling 37 as amended by inf-ap (2026-09-30): `inf ≈ inf` and `inf ≈ int`
    // are the integer-family tolerance `10^9·|a − b| ≤ max(|a|, |b|)`, the same
    // as `i32 ≈ i64` (`inf_approx_test.rs` pins the matrix); `inf ≈ float`
    // converts the `inf` side by ruling 40 and compares in the float.
    assert_eq!(
        ok("sit inf x ← 18446744073709551617\nsit inf y ← 18446744073709551616\nnota x ≈ y\nnota x ≉ y\nnota x ≈ x"),
        ["verum", "falsum", "verum"]
    );
    assert_eq!(
        ok(
            "sit inf x ← 18446744073709551617\nsit fractus f ← 1.8446744073709552e19\nnota x ≈ f\nnota f ≈ x\nsit numerus a ← 9007199254740993\nnota a ≈ f"
        ),
        ["verum", "verum", "falsum"]
    );
    assert_eq!(ok("sit inf a ← 1000000001\nsit inf b ← 1000000000\nnota a ≈ b"), ["verum"]);
    assert_eq!(ok("sit inf a ← 2000000000\nsit inf b ← 1000000000\nnota a ≈ b"), ["falsum"]);
}

#[test]
fn a_runaway_shift_ends_in_runner_inf_size_exhausted() {
    // ruling 45: a resource fault with its own identity, not a memory abort.
    let (_, issue, message) = trap("sit inf one ← 1\nvaria numerus n ← 4000000000\nnota one ⇐ n");
    assert_eq!(issue, "runner_inf_size_exhausted");
    assert!(message.contains("shift"), "{message}");
}

#[test]
fn a_runaway_potentia_ends_in_runner_inf_size_exhausted() {
    // ruling 45: the repeated-squaring loop reaches the runner ceiling and
    // stops with the fault instead of spending hours or aborting on memory.
    let (_, issue, message) = trap("sit inf three ← 3\nnota three.potentia(100000000000)");
    assert_eq!(issue, "runner_inf_size_exhausted");
    assert!(message.contains("multiplication"), "{message}");
}

#[test]
fn the_range_loop_binder_is_typed_by_its_bounds() {
    // ruling 43: an `inf` bound gives an `inf` binder, whose `+ step` is exact.
    assert_eq!(
        ok(&format!(
            "sit inf top ← {TWO_64}\nvaria inf count ← 0\nitera ab top‥top + 3 sit i {{\n nota i\n count ↑\n}}\nnota count"
        )),
        [
            "18446744073709551616",
            "18446744073709551617",
            "18446744073709551618",
            "3"
        ]
    );
    assert_eq!(
        ok("sit inf n ← 100\nvaria inf total ← 0\nitera ab 0…n sit i {\n total ← total + i\n}\nnota total"),
        ["5050"]
    );
    // Bounded bounds keep today's `numerus` binder.
    assert_eq!(
        ok("varia numerus total ← 0\nitera ab 0‥5 sit i {\n total ← total + i\n}\nnota total"),
        ["10"]
    );
}

#[test]
fn an_inf_beside_a_decimal_is_not_joined_at_d64() {
    // ruling 37: the d64 join would trap for a large value; the comparison is exact.
    assert_eq!(
        ok("sit inf huge ← 10 ⇐ 200\nsit d64 d ← 1.5\nnota huge ≻ d\nnota d ≺ huge\nnota huge ≥ 1.5"),
        ["verum", "verum", "verum"]
    );
}

#[test]
fn a_function_over_inf_recurses_past_the_cap() {
    let (out, errors) = run_items(
        "functio fact(inf n) → inf {\n si n ≤ 1 { redde 1 }\n redde n * fact(n - 1)\n}\n",
        "nota fact(25)",
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["15511210043330985984000000"]);
}

#[test]
fn a_big_literal_below_i128_and_beyond_lowers_to_one_constant() {
    // ruling 34: canonical, sign and radix spellings, every size.
    assert_eq!(
        ok(
            "sit inf a ← 0x10000000000000000\nsit inf b ← 0b1000000000000000000000000000000000000000000000000000000000000000\nsit inf c ← 9223372036854775808\nnota a\nnota b\nnota c"
        ),
        ["18446744073709551616", "9223372036854775808", "9223372036854775808"]
    );
    assert_eq!(
        ok(
            "sit inf z ← 100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000\nnota z"
        ),
        ["100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"]
    );
}
