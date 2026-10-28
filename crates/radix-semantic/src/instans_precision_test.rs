//! Tests for instans precision parsing and assignability.
//!
//! These tests prove that named precision markers (ms, us, ns) parse to the
//! correct `InstansPrecision` variant and that precision assignability rules
//! between ranks behave as expected.

use super::instans_precision::{
    InstansPrecision, InstansPrecisionError, InstansPrecisionRank, is_instans_assignable,
};
use crate::{Primitive, TypeTable};

#[test]
fn parses_named_precision_markers() {
    assert_eq!(
        InstansPrecision::from_marker("ms").ok(),
        Some(InstansPrecision::Millis)
    );
    assert_eq!(
        InstansPrecision::from_marker("us").ok(),
        Some(InstansPrecision::Micros)
    );
    assert_eq!(
        InstansPrecision::from_marker("ns").ok(),
        Some(InstansPrecision::Nanos)
    );
    assert_eq!(
        InstansPrecision::from_marker("s"),
        Err(InstansPrecisionError::UnknownMarker)
    );
}

#[test]
fn faber_marker_roundtrips_parsed_precisions() {
    for (marker, precision) in [
        ("ms", InstansPrecision::Millis),
        ("us", InstansPrecision::Micros),
        ("ns", InstansPrecision::Nanos),
    ] {
        assert_eq!(InstansPrecision::from_marker(marker).ok(), Some(precision));
        assert_eq!(precision.faber_marker(), marker);
    }
}

#[test]
fn instans_precision_widening_lattice() {
    assert!(InstansPrecisionRank::is_widening_assignable(
        InstansPrecisionRank::Seconds,
        InstansPrecisionRank::Millis
    ));
    assert!(InstansPrecisionRank::is_widening_assignable(
        InstansPrecisionRank::Millis,
        InstansPrecisionRank::Nanos
    ));
    assert!(!InstansPrecisionRank::is_widening_assignable(
        InstansPrecisionRank::Nanos,
        InstansPrecisionRank::Millis
    ));
}

#[test]
fn instans_assignable_in_type_table() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let nanos = types.sized_instans(InstansPrecision::Nanos);

    assert!(is_instans_assignable(&types, bare, millis));
    assert!(is_instans_assignable(&types, millis, nanos));
    assert!(!is_instans_assignable(&types, nanos, millis));
    assert!(!is_instans_assignable(&types, millis, bare));
}
