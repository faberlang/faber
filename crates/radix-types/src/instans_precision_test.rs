//! [`InstansPrecision`] marker parsing, faber-marker roundtrip, precision widening
//! lattice (coarse→fine assignable, fine→coarse not), and assignment in
//! [`TypeTable`].

use crate::{instans_assignable, InstansPrecision, InstansPrecisionError, InstansPrecisionRank};
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
fn widening_assignable_coarse_to_fine_seconds() {
    assert!(InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Seconds,
        InstansPrecisionRank::Millis
    ));
    assert!(InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Seconds,
        InstansPrecisionRank::Micros
    ));
}

#[test]
fn widening_assignable_coarse_to_fine_millis() {
    assert!(InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Millis,
        InstansPrecisionRank::Micros
    ));
    assert!(InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Millis,
        InstansPrecisionRank::Nanos
    ));
}

#[test]
fn widening_assignable_coarse_to_fine_micros() {
    assert!(InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Micros,
        InstansPrecisionRank::Nanos
    ));
}

#[test]
fn widening_assignable_fine_to_coarse_not_nanos() {
    assert!(!InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Nanos,
        InstansPrecisionRank::Micros
    ));
    assert!(!InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Nanos,
        InstansPrecisionRank::Millis
    ));
}

#[test]
fn widening_assignable_fine_to_coarse_not_micros() {
    assert!(!InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Micros,
        InstansPrecisionRank::Millis
    ));
    assert!(!InstansPrecisionRank::widening_assignable(
        InstansPrecisionRank::Micros,
        InstansPrecisionRank::Seconds
    ));
}

#[test]
fn instans_assignable_bare_to_sized() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let micros = types.sized_instans(InstansPrecision::Micros);
    let nanos = types.sized_instans(InstansPrecision::Nanos);
    assert!(instans_assignable(&types, bare, millis));
    assert!(instans_assignable(&types, bare, micros));
    assert!(instans_assignable(&types, bare, nanos));
}

#[test]
fn instans_assignable_sized_to_finer() {
    let mut types = TypeTable::new();
    let millis = types.sized_instans(InstansPrecision::Millis);
    let micros = types.sized_instans(InstansPrecision::Micros);
    let nanos = types.sized_instans(InstansPrecision::Nanos);
    assert!(instans_assignable(&types, millis, micros));
    assert!(instans_assignable(&types, millis, nanos));
    assert!(instans_assignable(&types, micros, nanos));
}

#[test]
fn instans_assignable_nanos_to_coarser_not() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let micros = types.sized_instans(InstansPrecision::Micros);
    let nanos = types.sized_instans(InstansPrecision::Nanos);
    assert!(!instans_assignable(&types, nanos, micros));
    assert!(!instans_assignable(&types, nanos, millis));
    assert!(!instans_assignable(&types, nanos, bare));
}

#[test]
fn instans_assignable_micros_or_millis_to_coarser_not() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let micros = types.sized_instans(InstansPrecision::Micros);
    assert!(!instans_assignable(&types, micros, millis));
    assert!(!instans_assignable(&types, micros, bare));
    assert!(!instans_assignable(&types, millis, bare));
}
