use super::{Instans, InstansPraecisio};
use std::cmp::Ordering;

#[test]
fn ord_agrees_with_eq_across_precisions() {
    let fine = Instans::from_nanos(1_703_000_000_123_456_000, InstansPraecisio::Nanosecunda);
    let coarse = Instans::from_nanos(1_703_000_000_123_999_000, InstansPraecisio::Millisecunda);

    assert_ne!(fine, coarse);
    assert_ne!(fine.cmp(&coarse), Ordering::Equal);
    assert_eq!(fine.partial_cmp(&coarse), Some(fine.cmp(&coarse)));
    assert_eq!(fine.partial_cmp_at_coarser(coarse), Some(Ordering::Equal));
    assert_eq!(fine.cmp_at_coarser(coarse), Ordering::Equal);
}

#[test]
fn ord_same_precision_orders_by_nanos() {
    let earlier = Instans::from_epoch_seconds(1, InstansPraecisio::Secunda);
    let later = Instans::from_epoch_seconds(2, InstansPraecisio::Secunda);

    assert_eq!(earlier.cmp(&later), Ordering::Less);
}

#[test]
fn equal_values_cmp_equal() {
    let instant = Instans::from_epoch_seconds(1, InstansPraecisio::Secunda);
    let copy = instant;

    assert_eq!(instant, copy);
    assert_eq!(instant.cmp(&copy), Ordering::Equal);
    assert_eq!(instant.partial_cmp(&copy), Some(Ordering::Equal));
}
