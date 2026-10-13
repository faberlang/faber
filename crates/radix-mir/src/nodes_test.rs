//! `MirConstant` Hash/Eq contract: values that compare equal hash equal.

use crate::MirConstant;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_of(constant: &MirConstant) -> u64 {
    let mut hasher = DefaultHasher::new();
    constant.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn signed_zero_float_constants_hash_equal_because_they_compare_equal() {
    let positive = MirConstant::Float(0.0);
    let negative = MirConstant::Float(-0.0);
    assert_eq!(positive, negative, "derived PartialEq treats ±0.0 as equal");
    assert_eq!(
        hash_of(&positive),
        hash_of(&negative),
        "equal constants must hash equal"
    );
}

#[test]
fn distinct_float_constants_keep_distinct_hashes() {
    assert_ne!(
        hash_of(&MirConstant::Float(0.0)),
        hash_of(&MirConstant::Float(1.0))
    );
}
