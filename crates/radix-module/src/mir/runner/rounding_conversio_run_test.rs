//! Need: `x ↦ int via Floor|Ceil|HalfEven|Trunc` on the MIR runner.
//! The `via` hint rides in the conversion's `params`; the runner rounds with
//! it and then applies the range and NaN check, so a value outside the target
//! or a NaN fails on the error channel and `⊥` recovers it.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_lines(name: &str, body: &str) -> Vec<String> {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}\n");
    let mut host = BufferHost::default();
    run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        &source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("{name} must run: {error:?}"));
    host.stdout_lines
}

/// One function per source family that prints the four modes of one value
/// into `i32`, in the order Floor, Ceil, HalfEven, Trunc.
const MODES: &str = r#"
fn float_modes(f64 x) → string {
    let i32 fl ← x ↦ i32 via Floor ⊥ 99
    let i32 ce ← x ↦ i32 via Ceil ⊥ 99
    let i32 he ← x ↦ i32 via HalfEven ⊥ 99
    let i32 tr ← x ↦ i32 via Trunc ⊥ 99
    return "§ § § §"(fl, ce, he, tr)
}

fn decimal_modes(d64 x) → string {
    let i32 fl ← x ↦ i32 via Floor ⊥ 99
    let i32 ce ← x ↦ i32 via Ceil ⊥ 99
    let i32 he ← x ↦ i32 via HalfEven ⊥ 99
    let i32 tr ← x ↦ i32 via Trunc ⊥ 99
    return "§ § § §"(fl, ce, he, tr)
}
"#;

#[test]
fn float_modes_round_ties_negatives_and_fractions() {
    let body = format!(
        "{MODES}\nmain {{\n\
         print float_modes(2.5)\n\
         print float_modes(3.5)\n\
         print float_modes(-2.5)\n\
         print float_modes(-0.5)\n\
         print float_modes(0.5)\n\
         print float_modes(3.7)\n\
         print float_modes(-3.7)\n\
         print float_modes(4.0)\n}}"
    );
    assert_eq!(
        run_lines("rounding-float-modes.fab", &body),
        [
            "2 3 2 2",
            "3 4 4 3",
            "-3 -2 -2 -2",
            "-1 0 0 0",
            "0 1 0 0",
            "3 4 4 3",
            "-4 -3 -4 -3",
            "4 4 4 4",
        ]
    );
}

#[test]
fn decimal_modes_round_ties_negatives_and_fractions() {
    let body = format!(
        "{MODES}\nmain {{\n\
         print decimal_modes(2.5)\n\
         print decimal_modes(3.5)\n\
         print decimal_modes(-2.5)\n\
         print decimal_modes(-0.5)\n\
         print decimal_modes(0.5)\n\
         print decimal_modes(3.7)\n\
         print decimal_modes(-3.7)\n\
         print decimal_modes(4.0)\n}}"
    );
    assert_eq!(
        run_lines("rounding-decimal-modes.fab", &body),
        [
            "2 3 2 2",
            "3 4 4 3",
            "-3 -2 -2 -2",
            "-1 0 0 0",
            "0 1 0 0",
            "3 4 4 3",
            "-4 -3 -4 -3",
            "4 4 4 4",
        ]
    );
}

#[test]
fn the_rounded_value_is_what_the_range_check_sees() {
    // 255.5 floors to 255 (fits u8) but rounds half-even to 256 (does not);
    // -0.5 truncates and half-even rounds to 0 (fits u8) but floors to -1.
    let body = r#"
main {
    let f64 high ← 255.5
    let f64 low ← -0.5
    print high ↦ u8 via Floor ⊥ 7
    print high ↦ u8 via Trunc ⊥ 7
    print high ↦ u8 via Ceil ⊥ 7
    print high ↦ u8 via HalfEven ⊥ 7
    print low ↦ u8 via Trunc ⊥ 7
    print low ↦ u8 via HalfEven ⊥ 7
    print low ↦ u8 via Floor ⊥ 7
    print low ↦ u8 via Ceil ⊥ 7
}
"#;
    assert_eq!(
        run_lines("rounding-range-check.fab", body),
        ["255", "255", "7", "7", "0", "0", "7", "0"]
    );
}

#[test]
fn out_of_range_and_nan_fail_into_the_default_for_every_mode() {
    let body = r#"
fn probe(f64 x) → string {
    let i32 fl ← x ↦ i32 via Floor ⊥ 99
    let i32 ce ← x ↦ i32 via Ceil ⊥ 99
    let i32 he ← x ↦ i32 via HalfEven ⊥ 99
    let i32 tr ← x ↦ i32 via Trunc ⊥ 99
    return "§ § § §"(fl, ce, he, tr)
}

main {
    let f64 zero ← 0.0
    let f64 notnum ← zero / zero
    let f64 plus ← 1.0 / zero
    let f64 minus ← -1.0 / zero
    print probe(notnum)
    print probe(plus)
    print probe(minus)
    print probe(3000000000.0)
    print probe(-3000000000.5)
}
"#;
    assert_eq!(
        run_lines("rounding-failure.fab", body),
        [
            "99 99 99 99",
            "99 99 99 99",
            "99 99 99 99",
            "99 99 99 99",
            "99 99 99 99"
        ]
    );
}

#[test]
fn large_decimals_fail_into_the_default_and_fit_into_inf() {
    let body = r#"
main {
    let d64 huge ← 3000000000.5
    print huge ↦ i32 via Floor ⊥ 99
    print huge ↦ i64 via Floor ⊥ 99
    print huge ↦ inf via Ceil ⊥ 99
    print huge ↦ inf via HalfEven ⊥ 99
}
"#;
    assert_eq!(
        run_lines("rounding-decimal-range.fab", body),
        ["99", "3000000000", "3000000001", "3000000000"]
    );
}

#[test]
fn inf_target_rounds_floats_and_decimals_by_the_clause() {
    let body = r#"
main {
    let f64 f ← -2.5
    let d64 d ← -2.5
    print f ↦ inf via Floor ⊥ 99
    print f ↦ inf via Ceil ⊥ 99
    print f ↦ inf via HalfEven ⊥ 99
    print f ↦ inf via Trunc ⊥ 99
    print d ↦ inf via Floor ⊥ 99
    print d ↦ inf via Ceil ⊥ 99
    print d ↦ inf via HalfEven ⊥ 99
    print d ↦ inf via Trunc ⊥ 99
}
"#;
    assert_eq!(
        run_lines("rounding-inf.fab", body),
        ["-3", "-2", "-2", "-2", "-3", "-2", "-2", "-2"]
    );
}

#[test]
fn policy_word_targets_round_then_apply_the_policy() {
    let body = r#"
main {
    let f64 f ← 300.5
    let d64 d ← 300.5
    print f ↦ wrapping<u8> via Floor
    print f ↦ saturating<u8> via Floor
    print d ↦ wrapping<u8> via Ceil
    print d ↦ saturating<u8> via Trunc
}
"#;
    assert_eq!(run_lines("rounding-policy-words.fab", body), ["44", "255", "45", "255"]);
}
