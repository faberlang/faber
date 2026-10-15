//! inf-ap (operator ruling 2026-09-30): `i32 ≈ inf` behaves exactly like
//! `i32 ≈ i64`. One rule for integer-family `≈`/`≉` (bounded, `u64` and `inf`):
//! the exact form of the relative 1e-9 tolerance, `10^9·|a − b| ≤ max(|a|, |b|)`,
//! computed in integers. A float or `d64` operand keeps computing in floats.
//!
//! These run end to end through the MIR runner (the oracle): the same Faber
//! values are declared at every integer type that holds them, and every
//! ordered type pair must give the reference answer, the mirrored answer
//! (symmetry) and the negation under `≉`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

/// One integer type of the family: Faber spelling, name, and whether a value fits.
type FamilyRow = (&'static str, &'static str, fn(i128) -> bool);

/// The integer types of the family.
const FAMILY: [FamilyRow; 4] = [
    ("i32", "i32", |v| i32::try_from(v).is_ok()),
    ("numerus", "i64", |v| i64::try_from(v).is_ok()),
    ("u64", "u64", |v| u64::try_from(v).is_ok()),
    ("inf", "inf", |_| true),
];

fn run(body: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "inf_approx.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Stepper(errors)) => panic!("{body}: {errors:?}"),
        Err(other) => panic!("{body}: {other:?}"),
    }
}

/// The reference: `10^9·|a − b| ≤ max(|a|, |b|)` in `i128` (inputs stay below
/// 2^96 so the product cannot overflow).
fn reference(a: i128, b: i128) -> bool {
    1_000_000_000 * (a - b).abs() <= a.abs().max(b.abs())
}

/// Every applicable ordered pair of family types for `a ≈ b`: for each, the
/// answers of `a ≈ b`, `a ≉ b` and `b ≈ a`.
fn matrix(a: i128, b: i128) -> Vec<(&'static str, &'static str, [bool; 3])> {
    let mut pairs = Vec::new();
    let mut body = String::new();
    for (ta, na, fits_a) in FAMILY {
        for (tb, nb, fits_b) in FAMILY {
            if fits_a(a) && fits_b(b) {
                let i = pairs.len();
                body += &format!("sit {ta} a{i} ← {a}\nsit {tb} b{i} ← {b}\n");
                body += &format!("nota a{i} ≈ b{i}\nnota a{i} ≉ b{i}\nnota b{i} ≈ a{i}\n");
                pairs.push((na, nb));
            }
        }
    }
    let out = run(&body);
    assert_eq!(out.len(), pairs.len() * 3, "{a} ≈ {b}: {out:?}");
    pairs
        .into_iter()
        .enumerate()
        .map(|(i, (na, nb))| {
            let bit = |k: usize| out[3 * i + k] == "verum";
            (na, nb, [bit(0), bit(1), bit(2)])
        })
        .collect()
}

/// Every pair of types that holds both values agrees with the reference, with
/// its mirror, and with the negation.
fn assert_family_agrees(a: i128, b: i128) {
    let expected = reference(a, b);
    let cells = matrix(a, b);
    assert!(!cells.is_empty());
    for (na, nb, [approx, not_approx, mirrored]) in cells {
        assert_eq!(approx, expected, "{na} {a} ≈ {nb} {b}");
        assert_eq!(not_approx, !expected, "{na} {a} ≉ {nb} {b} is the negation");
        assert_eq!(mirrored, expected, "{nb} {b} ≈ {na} {a} is symmetric");
    }
}

#[test]
fn small_and_tolerance_boundary_values_agree_across_the_family() {
    // Relative 1e-9: 5 vs 6 is far, 10^12 vs 10^12+1 is inside, the
    // boundary `10^9·1 ≤ 10^9` is inside, one more is outside.
    for (a, b) in [
        (5, 6),
        (0, 0),
        (0, 1),
        (7, 7),
        (-5, -6),
        (1_000_000_000_000, 1_000_000_000_001),
        (-1_000_000_000_000, -1_000_000_000_001),
        (1_000_000_000, 1_000_000_001),
        (999_999_999, 1_000_000_000),
        (999_999_999, 1_000_000_001),
        (1_000_000_000_000_000, 1_000_000_001_000_000),
        (1_000_000_000_000_000, 1_000_000_001_000_001),
    ] {
        assert_family_agrees(a, b);
    }
}

#[test]
fn values_beyond_2_pow_53_compare_exactly_not_through_f64() {
    // f64 rounds these; the integer rule does not. The first pair is inside
    // tolerance under f64 rounding but outside exactly; the second the reverse.
    assert_family_agrees(16_728_793_246_425_917, 16_728_793_229_697_123);
    assert_family_agrees(40_165_110_621_015_797, 40_165_110_580_850_688);
    assert!(!reference(16_728_793_246_425_917, 16_728_793_229_697_123));
    assert!(reference(40_165_110_621_015_797, 40_165_110_580_850_688));
    // 2^53 + 1 vs 2^53 and the 64-bit edges.
    assert_family_agrees(9_007_199_254_740_993, 9_007_199_254_740_992);
    assert_family_agrees(i128::from(i64::MAX), i128::from(i64::MAX) - 1);
    assert_family_agrees(i128::from(u64::MAX), i128::from(u64::MAX) - 1);
}

#[test]
fn inf_beyond_64_bits_uses_the_same_rule() {
    let two_64 = 1_i128 << 64;
    assert_family_agrees(two_64 + 1, two_64);
    assert_family_agrees(two_64, two_64 + two_64 / 1_000_000_000);
    assert_family_agrees(two_64, two_64 + two_64 / 1_000_000_000 * 2);
    // Beyond i128: one inf of 10⋅2^400 against itself plus one.
    let body = "sit inf a ← 10 ⇐ 400\nsit inf b ← a + 1\nsit inf c ← a + a\n\
                nota a ≈ b\nnota b ≈ a\nnota a ≉ b\nnota a ≈ c\nnota c ≈ a\nnota a ≉ c";
    assert_eq!(run(body), ["verum", "verum", "falsum", "falsum", "falsum", "verum"]);
}

#[test]
fn inf_against_a_float_or_d64_keeps_the_float_joined_width() {
    let body = "sit inf x ← 18446744073709551617\nsit fractus f ← 1.8446744073709552e19\n\
                nota x ≈ f\nnota f ≈ x\n\
                sit inf m ← 1000000000000\nsit fractus g ← 1000000000001.0\nnota m ≈ g\nnota g ≈ m\n\
                sit inf five ← 5\nsit fractus six ← 6.0\nnota five ≈ six";
    assert_eq!(run(body), ["verum", "verum", "verum", "verum", "falsum"]);
    // `inf` beside `d64` is a float-width tolerance like `i64` beside `d64`; the
    // decimal carrier is unscaled, never read as an integer.
    let d64 = "sit inf seven ← 7\nsit numerus sevenI ← 7\nsit d64 d ← 7.0\n\
               nota seven ≈ d\nnota d ≈ seven\nnota sevenI ≈ d\nnota seven ≉ d\n\
               sit inf m ← 10000000000\nsit numerus mi ← 10000000000\nsit d64 e ← 10000000001.0\n\
               nota m ≈ e\nnota mi ≈ e\n\
               sit inf five ← 5\nsit d64 six ← 6.0\nnota five ≈ six";
    assert_eq!(run(d64), ["verum", "verum", "verum", "falsum", "verum", "verum", "falsum"]);
}
