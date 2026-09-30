//! d11-6 ruling 19 range table over declared widths.

use crate::NumericWidth::{self, I8, I16, I32, I64, U8, U16, U32, U64};
use crate::numeric_range::{self as r, IntRange};

fn of(width: NumericWidth) -> IntRange {
    width.integer_range().unwrap()
}

#[test]
fn u8_table_matches_ruling_19() {
    let a = of(U8);
    assert_eq!(r::smallest_width(r::add(a, a)), U16);
    assert_eq!(r::smallest_width(r::mul(a, a)), U16);
    assert_eq!(r::smallest_width(r::sub(a, a)), I16);
    assert_eq!(r::smallest_width(r::neg(a)), I16);
    assert_eq!(r::smallest_width(r::bit_not(a)), I16);
    assert_eq!(r::smallest_width(r::floor_div(a, a)), U8);
    assert_eq!(r::smallest_width(r::floor_rem(a, a)), U8);
    assert_eq!(r::smallest_width(r::bit_and(a, (255, 255))), U8);
    assert_eq!(r::smallest_width(r::hull(a, a)), U8);
}

#[test]
fn sixty_four_bit_cap_keeps_the_sign() {
    assert_eq!(r::smallest_width(r::sub(of(U64), of(U64))), I64);
    assert_eq!(r::smallest_width(r::add(of(U64), of(U64))), U64);
    assert_eq!(r::smallest_width(r::mul(of(U64), of(U64))), U64);
    assert_eq!(r::smallest_width(r::add(of(I64), of(I64))), I64);
    assert_eq!(r::smallest_width(r::sub(of(U64), (1, 1))), I64);
    assert_eq!(r::smallest_width(r::sub(of(U32), of(U32))), I64);
    assert_eq!(r::smallest_width(r::sub(of(U16), of(U16))), I32);
}

#[test]
fn constants_take_part_by_value() {
    assert_eq!(r::smallest_width(r::add(of(U8), (1, 1))), U16);
    assert_eq!(r::smallest_width(r::add((10, 10), (-100, -100))), I8);
    assert_eq!(r::smallest_width(r::sub(of(I8), (1, 1))), I16);
}

#[test]
fn remainder_and_shift_ranges() {
    assert_eq!(r::smallest_width(r::floor_rem(of(U8), (10, 10))), U8);
    assert_eq!(r::floor_rem(of(I8), (-7, 7)), (-6, 6));
    assert_eq!(r::smallest_width(r::shl(of(U8), (4, 4))), U16);
    assert_eq!(r::smallest_width(r::shl(of(U8), of(U8))), U64);
    assert_eq!(r::smallest_width(r::shl(of(I8), of(U8))), I64);
}

#[test]
fn container_check() {
    assert!(r::fits_container(of(I64)));
    assert!(r::fits_container(of(U64)));
    assert!(!r::fits_container(r::sub(of(U64), (-1, -1))));
    assert!(r::fits_container((-1, i128::from(u64::MAX))));
    assert!(!r::fits_container((i128::from(i64::MIN) - 1, 0)));
    assert_eq!(r::smallest_width((0, 0)), U8);
    assert_eq!(r::smallest_width((0, 70_000)), U32);
}

#[test]
fn abs_and_power_ranges_are_exact() {
    // ss1: `|x|` and `x ^ n` are typed by their exact interval.
    assert_eq!(r::abs(of(I8)), (0, 128));
    assert_eq!(r::abs((-5, -2)), (2, 5));
    assert_eq!(r::abs((3, 9)), (3, 9));
    assert_eq!(r::smallest_width(r::abs(of(I64))), U64);
    assert_eq!(r::pow_const(of(I32), 2), (0, 1 << 62));
    assert_eq!(r::pow_const((-3, 2), 3), (-27, 8));
    assert_eq!(r::pow_const((-3, 2), 2), (0, 9));
    assert_eq!(r::pow_const((-4, -2), 2), (4, 16));
    assert_eq!(r::pow_const(of(U8), 0), (1, 1));
    assert_eq!(r::smallest_width(r::pow_const(of(U8), 3)), U32);
    assert_eq!(r::smallest_width(r::pow_unbounded(of(I8))), I64);
    assert_eq!(r::smallest_width(r::pow_unbounded(of(U8))), U64);
}
