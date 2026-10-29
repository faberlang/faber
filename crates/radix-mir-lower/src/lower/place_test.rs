use super::*;

#[test]
fn literal_range_enumeration_stops_on_overflow_and_keeps_inclusive_endpoint() {
    assert_eq!(enumerate_literal_range(1, 4, i128::MAX, false), Some(vec![1]));
    assert_eq!(enumerate_literal_range(i128::MAX, i128::MAX, 1, true), Some(vec![i128::MAX]));
    assert_eq!(enumerate_literal_range(0, 4, 0, false), None);
}
